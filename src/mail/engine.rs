//! The bridge between the UI thread and the async mail workers.
//!
//! Freya's event loop is not a tokio runtime, so all network I/O runs on its own thread
//! with a small multi-threaded tokio runtime. The UI talks to it with `Engine::send`
//! (commands in) and drains `Engine::events` (events out) from a Freya task.

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use super::{
    config::{self, AccountConfig},
    imap, Command, Event, WorkerCmd,
};

#[derive(Clone)]
pub struct Engine {
    tx: Option<UnboundedSender<Command>>,
    pub events: async_channel::Receiver<Event>,
}

impl Engine {
    /// Engine that does nothing (demo mode). Its event channel is already closed.
    pub fn offline() -> Self {
        let (_tx, rx) = async_channel::unbounded();
        Engine { tx: None, events: rx }
    }

    /// Start the I/O thread and connect every saved account.
    pub fn start(accounts: Vec<AccountConfig>) -> Self {
        let (event_tx, event_rx) = async_channel::unbounded();
        let (cmd_tx, cmd_rx) = unbounded_channel();

        std::thread::Builder::new()
            .name("photon-mail-io".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .thread_name("photon-mail-rt")
                    .build()
                    .expect("failed to start the mail runtime");
                runtime.block_on(manager(accounts, cmd_rx, event_tx));
            })
            .expect("failed to spawn the mail I/O thread");

        Engine { tx: Some(cmd_tx), events: event_rx }
    }

    pub fn send(&self, cmd: Command) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
    }

    pub fn account(&self, account: usize, cmd: WorkerCmd) {
        self.send(Command::Account { account, cmd });
    }
}

/// Owns the per-account workers and handles "add account".
async fn manager(
    accounts: Vec<AccountConfig>,
    mut cmd_rx: UnboundedReceiver<Command>,
    events: async_channel::Sender<Event>,
) {
    let mut configs = accounts.clone();
    // Worker position == account index in the UI's store.
    let mut workers: Vec<UnboundedSender<WorkerCmd>> = Vec::new();
    for (index, cfg) in accounts.into_iter().enumerate() {
        workers.push(imap::spawn_account(index, cfg, None, events.clone()));
    }

    type Verified = (AccountConfig, String, Result<(), String>);
    let (done_tx, mut done_rx) = unbounded_channel::<Verified>();

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else { break };
                match cmd {
                    Command::Account { account, cmd } => {
                        if let Some(worker) = workers.get(account) {
                            let _ = worker.send(cmd);
                        }
                    }
                    Command::AddAccount { cfg, password } => {
                        let duplicate = configs.iter().any(|c| {
                            c.email.eq_ignore_ascii_case(&cfg.email)
                                && c.host.eq_ignore_ascii_case(&cfg.host)
                        });
                        if duplicate {
                            let _ = events.try_send(Event::AddAccountFailed {
                                error: "That account is already added.".to_string(),
                            });
                            continue;
                        }
                        // Verify off to the side so the manager stays responsive.
                        let done_tx = done_tx.clone();
                        tokio::spawn(async move {
                            let result = imap::verify(&cfg, &password).await;
                            let _ = done_tx.send((cfg, password, result));
                        });
                    }
                    Command::UpdateAccount { index, cfg } => {
                        if let Some(existing) = configs.get_mut(index) {
                            *existing = cfg.clone();
                            if let Err(e) = config::save_accounts(&configs) {
                                let _ = events.try_send(Event::Notice {
                                    text: format!("Couldn't save the account list: {e}"),
                                });
                            }
                        }
                        // The running worker keeps its own copy of the config; without this the
                        // new SMTP settings would only take effect after a restart.
                        if let Some(worker) = workers.get(index) {
                            let _ = worker.send(WorkerCmd::UpdateConfig { cfg });
                        }
                    }
                }
            }

            Some((cfg, password, result)) = done_rx.recv() => {
                if let Err(error) = result {
                    let _ = events.try_send(Event::AddAccountFailed { error });
                    continue;
                }

                // Credentials work. Keep the password in the OS keyring, never on disk.
                let (id, secret) = (cfg.id.clone(), password.clone());
                let stored = tokio::task::spawn_blocking(move || config::save_password(&id, &secret))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
                if let Err(e) = stored {
                    let _ = events.try_send(Event::AddAccountFailed {
                        error: format!("Couldn't save the password to the system keyring: {e}"),
                    });
                    continue;
                }

                configs.push(cfg.clone());
                if let Err(e) = config::save_accounts(&configs) {
                    let _ = events.try_send(Event::Notice {
                        text: format!("Couldn't save the account list: {e}"),
                    });
                }

                // Tell the UI about the account *before* its worker starts talking,
                // so the store always knows the index events refer to.
                let index = workers.len();
                let _ = events.try_send(Event::AccountAdded { cfg: cfg.clone() });
                workers.push(imap::spawn_account(index, cfg, Some(password), events.clone()));
            }
        }
    }
}
