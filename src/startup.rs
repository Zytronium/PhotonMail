use std::{fs, io, path::Path};

#[cfg(any(test, target_os = "macos"))]
const APP_ID: &str = "dev.zytronium.PhotonMail";

#[cfg(target_os = "linux")]
pub fn set_enabled(enabled: bool) -> io::Result<()> {
    let path = dirs::config_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "config directory unavailable"))?
        .join("autostart")
        .join("photon-mail.desktop");
    if enabled {
        let executable = std::env::current_exe()?;
        fs::create_dir_all(path.parent().expect("autostart file has a parent"))?;
        fs::write(&path, linux_desktop_entry(&executable))
    } else {
        remove_if_exists(&path)
    }
}

#[cfg(target_os = "macos")]
pub fn set_enabled(enabled: bool) -> io::Result<()> {
    let path = dirs::home_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory unavailable"))?
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{APP_ID}.plist"));
    if enabled {
        let executable = std::env::current_exe()?;
        fs::create_dir_all(path.parent().expect("launch agent has a parent"))?;
        fs::write(&path, macos_launch_agent(&executable))
    } else {
        remove_if_exists(&path)
    }
}

#[cfg(target_os = "windows")]
pub fn set_enabled(enabled: bool) -> io::Result<()> {
    let path = dirs::data_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "app data directory unavailable"))?
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("Startup")
        .join("Photon Mail.cmd");
    if enabled {
        let executable = std::env::current_exe()?;
        fs::create_dir_all(path.parent().expect("startup command has a parent"))?;
        fs::write(&path, windows_startup_command(&executable))
    } else {
        remove_if_exists(&path)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn set_enabled(_enabled: bool) -> io::Result<()> {
    Ok(())
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(any(test, target_os = "linux"))]
fn linux_desktop_entry(executable: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Photon Mail\nComment=Email client\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        desktop_exec_arg(executable)
    )
}

#[cfg(any(test, target_os = "linux"))]
fn desktop_exec_arg(path: &Path) -> String {
    let path = path.to_string_lossy();
    format!(
        "\"{}\"",
        path.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    )
}

#[cfg(any(test, target_os = "macos"))]
fn macos_launch_agent(executable: &Path) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n    <key>Label</key>\n    <string>{APP_ID}</string>\n    <key>ProgramArguments</key>\n    <array>\n        <string>{}</string>\n    </array>\n    <key>RunAtLoad</key>\n    <true/>\n</dict>\n</plist>\n",
        xml_escape(&executable.to_string_lossy())
    )
}

#[cfg(any(test, target_os = "macos"))]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(any(test, target_os = "windows"))]
fn windows_startup_command(executable: &Path) -> String {
    let executable = executable.to_string_lossy().replace('%', "%%");
    format!("@echo off\r\nstart \"\" \"{executable}\"\r\n")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        desktop_exec_arg, linux_desktop_entry, macos_launch_agent, windows_startup_command,
    };

    #[test]
    fn startup_entries_quote_executable_paths() {
        let executable = Path::new(r#"C:\Users\A&B\Photon%Mail\app.exe"#);

        let exec_arg = desktop_exec_arg(executable);
        assert!(exec_arg.starts_with('"') && exec_arg.ends_with('"'));
        assert!(exec_arg.contains("Photon%%Mail"));
        assert!(linux_desktop_entry(executable).contains("Exec=\"C:\\\\Users"));
        assert!(
            macos_launch_agent(Path::new("/Apps/Photon & Mail.app"))
                .contains("Photon &amp; Mail.app")
        );
        assert!(windows_startup_command(executable).contains("Photon%%Mail"));
    }
}
