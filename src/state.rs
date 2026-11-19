use freya::prelude::*;

use crate::data::{Mail, ACCOUNTS, MAILS};
use crate::keyboard::FocusState;

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    Mail,
    AddAccount,
    Settings(usize),
}

#[derive(Clone, Copy, PartialEq)]
pub struct AppState {
    pub account: State<Option<usize>>, // None = all accounts
    pub folder: State<usize>,
    pub selected: State<usize>,
    pub unread: State<Vec<bool>>,
    pub mail_folder: State<Vec<usize>>, // current folder of each mail
    pub expanded: State<Vec<bool>>,     // per-account sidebar group
    pub view: State<View>,
    pub unread_only: State<bool>,
    pub sidebar_width: State<f32>,
    pub message_list_width: State<f32>,
    pub focus_state: State<FocusState>, // keyboard focus state
}

pub fn use_app_state() -> AppState {
    AppState {
        account: use_state(|| None),
        folder: use_state(|| 0),
        selected: use_state(|| 0),
        unread: use_state(|| MAILS.iter().map(|m| m.unread).collect()),
        mail_folder: use_state(|| MAILS.iter().map(|m| m.folder).collect()),
        expanded: use_state(|| vec![true; ACCOUNTS.len()]),
        view: use_state(|| View::Mail),
        unread_only: use_state(|| false),
        sidebar_width: use_state(|| 260.),
        message_list_width: use_state(|| 380.),
        focus_state: use_state(|| FocusState::default()),
    }
}

impl AppState {
    fn matches_account(&self, m: &Mail) -> bool {
        let a = *self.account.read();
        a.map_or(true, |a| m.account == a)
    }

    /// Mails in the current account + folder, after the unread filter, newest first.
    pub fn visible(&self) -> Vec<usize> {
        let folder = *self.folder.read();
        let unread_only = *self.unread_only.read();
        let unread = self.unread.read();
        let mf = self.mail_folder.read();
        let mut v: Vec<usize> = (0..MAILS.len())
            .filter(|&i| {
                self.matches_account(&MAILS[i]) && mf[i] == folder && (!unread_only || unread[i])
            })
            .collect();
        v.sort_by_key(|&i| std::cmp::Reverse(MAILS[i].ts));
        v
    }

    /// The selected mail, if it's in the current view.
    pub fn open(&self) -> Option<usize> {
        let sel = *self.selected.read();
        self.visible().contains(&sel).then_some(sel)
    }

    pub fn is_unread(&self, i: usize) -> bool {
        self.unread.read()[i]
    }

    /// Unread mails in one folder of one account.
    pub fn account_folder_unread(&self, a: usize, f: usize) -> usize {
        let unread = self.unread.read();
        let mf = self.mail_folder.read();
        (0..MAILS.len())
            .filter(|&i| MAILS[i].account == a && mf[i] == f && unread[i])
            .count()
    }

    /// Unread inbox mails for one account, or all accounts for `None`.
    pub fn inbox_unread(&self, a: Option<usize>) -> usize {
        let unread = self.unread.read();
        let mf = self.mail_folder.read();
        (0..MAILS.len())
            .filter(|&i| mf[i] == 0 && a.map_or(true, |x| MAILS[i].account == x) && unread[i])
            .count()
    }

    pub fn go(&mut self, account: Option<usize>, folder: usize) {
        self.account.set(account);
        self.folder.set(folder);
        self.view.set(View::Mail);
    }

    pub fn select(&mut self, i: usize) {
        self.view.set(View::Mail);
        self.selected.set(i);
        self.unread.write()[i] = false;
    }

    pub fn toggle_expanded(&mut self, a: usize) {
        let v = !self.expanded.read()[a];
        self.expanded.write()[a] = v;
    }

    pub fn archive(&mut self, i: usize) {
        self.mail_folder.write()[i] = 3;
    }

    pub fn trash(&mut self, i: usize) {
        self.mail_folder.write()[i] = 5;
    }

    pub fn toggle_read(&mut self, i: usize) {
        let v = !self.unread.read()[i];
        self.unread.write()[i] = v;
    }
}
