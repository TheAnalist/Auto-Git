pub use log::*;
use powershell_script::{PsScript, PsScriptBuilder};
use rfd::FileDialog;
use std::path::Prefix::*;
use std::{
    fmt::{self, Write},
    fs, io,
    net::TcpStream,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process,
};
use win_msgbox::{Okay, error};

pub const MAX_TERMINAL_LENGHT: usize = 1000;
const CREATE_NO_WINDOW: u32 = 0x08000000;

const GIT_PROJECT_PATH: &str = ".git-project";
const STARTUP_DIR: &str = "($env:APPDATA + '\\Auto-Git\\')";

// ---------------------------------- GIT_COMMANDS ----------------------------------
//                          Autor: https://github.com/Timmmm
//                      Heavily modified due to security problems
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Process(ProcessError),
}

#[derive(Debug)]
pub struct ProcessError {
    output: process::Output,
    command: Vec<String>,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self {
            Self::Io(e) => e.fmt(f),
            Self::Process(e) => {
                write!(
                    f,
                    "process exited with exit code {}\nCommand: {:?}\nStdout: {}\nStderr: {}\n",
                    e.output.status,
                    e.command,
                    String::from_utf8_lossy(&e.output.stdout),
                    String::from_utf8_lossy(&e.output.stderr),
                )
            }
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Funzione di sicurezza per evitare connessione a cartelle UNC e leak del hash NTLM.
fn is_unsafe_path(path: &Path) -> bool {
    if let Some(s) = path.to_str() {
        // All known Windows UNC and device path prefixes
        let blocked = [
            "\\\\",
            "//",
            "\\\\?\\UNC",
            "\\\\?\\unc",
            "\\\\.\\",
            "\\??\\",
        ];
        if blocked.iter().any(|prefix| s.starts_with(prefix)) {
            return true;
        }
    }
    // Also catch via Rust's own path component analysis
    if let Some(std::path::Component::Prefix(p)) = path.components().next() {
        match p.kind() {
            UNC(_, _) | VerbatimUNC(_, _) | DeviceNS(_) => return true,
            _ => {}
        }
    }
    false
}

fn git(args: &[&str], working_dir: Option<&Path>) -> Result<process::Output, Error> {
    let mut command = process::Command::new("git");
    command.creation_flags(CREATE_NO_WINDOW);

    if let Some(raw_path) = working_dir {
        // security check: UNC paths
        if is_unsafe_path(&raw_path) {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "UNC and device paths are not allowed",
            )));
        }
    }

    if let Some(working_dir) = working_dir {
        // security check: real path
        let canonicalized_path = fs::canonicalize(working_dir).map_err(|e| Error::Io(e))?;
        // security check: contains .git folder
        let git_path = canonicalized_path.join(".git");
        // security check: path it's not a symlink
        if git_path.is_symlink() {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "Symlinked .git directory is not allowed",
            )));
        }

        if !git_path.is_dir() {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::NotFound,
                "No .git directory found for the project",
            )));
        }
        command.current_dir(canonicalized_path);
    }

    let output = command.args(args).output()?;

    if !output.status.success() {
        return Err(Error::Process(ProcessError {
            output,
            command: std::iter::once(&"git")
                .chain(args.iter())
                .map(|&s| s.to_owned())
                .collect(),
        }));
    }

    Ok(output)
}

// ---------------------------------- GIT_COMMANDS ----------------------------------

pub enum ChangesStaged {
    Staged,
    Ingored,
}

/// ## Lib Powershell PsScript Builder
///
/// crea un "component" PsScript con le preferenze di esecuzione
#[inline]
pub fn lib_pwsh_psscript_builder() -> PsScript {
    PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build()
}

#[allow(unused)]
/// Funzione di sicurezza: TOCTOU
pub fn lib_is_git_locked(project_path: &Option<PathBuf>) -> bool {
    if let Some(path) = project_path {
        path.join(".git").join("index.lock").exists()
    } else {
        false
    }
}

#[allow(unused)]
pub fn lib_cleanup_stale_lock(project_path: &Option<PathBuf>) {
    if let Some(project_path) = project_path {
        let lock_path = project_path.join(".git").join("index.lock");

        if lock_path.exists() {
            // Only remove if older than 60s — a fresh lock means git is running
            if let Ok(metadata) = fs::metadata(&lock_path) {
                if let Ok(modified) = metadata.modified() {
                    if modified.elapsed().unwrap_or_default().as_secs() > 60 {
                        let _ = fs::remove_file(&lock_path);
                    }
                }
            }
        }
    }
}

#[allow(unused)]
pub fn lib_check_internet() -> bool {
    info!(target: "lib", "checking internet connection");

    // handle powershell error
    match TcpStream::connect("8.8.8.8:53") {
        Ok(_) => {
            info!(target: "lib", "connected to internet!");
            true
        }
        Err(err) => {
            warn!(target: "lib","unable to connect to internet: {err}");
            false
        }
    }
}

#[allow(unused)]
pub fn lib_get_git_project_file_path() -> Option<String> {
    let ps: PsScript = lib_pwsh_psscript_builder();
    let mut path: String = String::with_capacity(STARTUP_DIR.len() + GIT_PROJECT_PATH.len());

    match ps.run(STARTUP_DIR) {
        Ok(output) => {
            write!(
                path,
                "{}{GIT_PROJECT_PATH}",
                output.stdout().unwrap().trim_end().to_string()
            )
            .unwrap();
            Some(path)
        }
        Err(err) => {
            error::<Okay>(&err.to_string()).show().unwrap();
            error!(target: "lib", "{}", err);
            None
        }
    }
}
// #[inline]
#[allow(unused)]
pub fn lib_get_project_path() -> Option<PathBuf> {
    info!(target: "lib", "getting git project path");

    let mut path: String = String::with_capacity(STARTUP_DIR.len() + GIT_PROJECT_PATH.len());
    // let ps: PsScript = lib_pwsh_psscript_builder();

    // handle powershell error
    match lib_get_git_project_file_path() {
        Some(output) => path = output,
        None => {
            return None;
        }
    }

    let fcontent = fs::read(&path);

    match fcontent {
        Ok(content) => {
            let project_path = content.iter().map(|c| *c as char).collect::<String>();

            if !project_path.is_empty() {
                Some(PathBuf::from(project_path))
            } else {
                warn!(target: "lib", "project path empty");

                let project_path = FileDialog::new()
                    .set_can_create_directories(true)
                    .set_title("Select project path")
                    .pick_folder();

                if let Some(ref pp) = project_path {
                    fs::write(&path, pp.display().to_string()).unwrap();
                }

                project_path
            }
        }
        Err(err) => {
            error!(target: "lib", "{err}");
            info!(target: "lib", "creating .git_project file");
            fs::File::create(&path).unwrap();
            None
        }
    }
}

/// aggigorna il progetto locale utilizzando "git remote update" e ritorna lo status
#[allow(unused)]
pub fn lib_git_update_local(project_path: &Option<PathBuf>) -> Option<String> {
    // Handle PathBuf None error
    if let Some(pb) = project_path {
        // fetch
        info!(target: "lib", "fetching");
        match git(&["fetch"], Some(pb.as_path())) {
            Ok(_) => {}
            Err(err) => {
                error::<Okay>(&err.to_string()).show().unwrap();
                error!(target: "lib", "{}", err);
                return None;
            }
        }
        // remote update
        info!(target: "lib", "remote update");
        match git(&["remote", "update"], Some(pb.as_path())) {
            Ok(_) => {}
            Err(err) => {
                if !err.to_string().contains("->") {
                    error::<Okay>(&err.to_string()).show().unwrap();
                    error!(target: "lib", "{}", err);
                    return None;
                }
            }
        };
        // status
        info!(target: "lib", "getting project status");
        match git(&["status"], Some(pb.as_path())) {
            Ok(out) => Some(out.stdout.iter().map(|c| *c as char).collect::<String>()),
            Err(err) => {
                error::<Okay>(&err.to_string()).show().unwrap();
                error!(target: "lib", "{}", err);
                None
            }
        }
    } else {
        error::<Okay>("Project path not selected").show().unwrap();
        error!(target: "lib", "project path not selected");
        None
    }
}

#[allow(unused)]
pub fn lib_git_status(project_path: &Option<PathBuf>) -> Option<String> {
    if let Some(pb) = project_path {
        info!(target: "lib", "getting project status");
        match git(&["status"], Some(pb.as_path())) {
            Ok(out) => return Some(out.stdout.iter().map(|c| *c as char).collect::<String>()),
            Err(err) => {
                error::<Okay>(&err.to_string()).show().unwrap();
                error!(target: "lib", "{}", err);
                return None;
            }
        }
    }

    None
}

#[allow(unused)]
pub fn lib_check_remote_ahead(status_string: String) -> bool {
    info!(target: "lib", "check remote ahead");

    if !status_string.contains("Your branch is up to date with")
        && !status_string.contains("Your branch is ahead of")
    {
        warn!(target: "lib", "remote project ahead");
        true
    } else {
        info!(target: "lib", "project aligned");
        false
    }
}

#[allow(unused)]
pub fn lib_make_pull(
    project_path: &Option<PathBuf>,
    // terminal_output: &mut Vec<String>,
) -> Result<(), Error> {
    info!(target: "lib", "pulling from remote repository");

    // if terminal_output.len() > MAX_TERMINAL_LENGHT {
    //     terminal_output.remove(0);
    // }

    // terminal_output.push("▶ Processing pull form remote repository...".to_string());

    // sono sicuro che project_path sia Some
    let path = project_path.as_ref().unwrap().display().to_string();

    // let mut command = String::with_capacity(path.len() + "git pull".len());
    // write!(command, "cd {path};git pull").unwrap();

    match git(&["pull"], Some(path.as_ref())) {
        Ok(_) => {
            info!(target: "lib", "pull completed with success");
            Ok(())
        }
        Err(err) => {
            error!(target: "lib", "{err}");
            error::<Okay>(
                &format!("Could not execute pull, check your internet connection or the selected project path\n{err}"))
                .show()
                .unwrap();
            Err(err)
        }
    }
}

#[allow(unused)]
pub fn lib_stage_changes(
    project_path: &Option<PathBuf>,
    files_staged: &mut Vec<String>,
) -> Result<ChangesStaged, Error> {
    info!(target: "lib","staging changes for commit");

    if let Some(project_path) = project_path {
        match git(&["status", "-s"], Some(project_path.as_path())) {
            Ok(out) => {
                let outputstr: String = out.stdout.iter().map(|c| *c as char).collect::<String>();
                /*
                Format (outputstr.split_whitespace):
                [src\lib.rs:274:17] files_vec = [
                    "M",
                    "auto-git/src/lib.rs",
                    "??",
                    "auto-git/Cargo.lock",
                ]
                */
                /*
                Alternativa ancora più efficiente: tuple_windows (con itertools)
                Se vuoi evitare del tutto di creare il primo Vec intermedio e lavorare in modo puramente
                "lazy" (pigro), potresti usare la libreria itertools, ma restando sulle funzioni standard
                di Rust, la soluzione sotto è la più leggibile.
                */
                let mut modified_files_vec: Vec<&str> = outputstr
                    .split_terminator("\n")
                    .filter(|f| f.contains("M ") || f.contains("MM ") || f.contains(" M"))
                    .map(|m_file| m_file.split_at(3).1)
                    .collect();

                // let mut modified_files_vec: Vec<&str> = outputstr
                //     .split_whitespace()
                //     .collect::<Vec<&str>>()
                //     .windows(2)
                //     .filter(|modified_char| modified_char[0] == "M" || modified_char[0] == "MM")
                //     .map(|file| file[1])
                //     .collect();

                // evita di fare il git add se i files sono uguali
                if modified_files_vec
                    .iter()
                    .all(|file| files_staged.contains(&file.to_string()))
                {
                    // files_staged.clear();
                    return Ok(ChangesStaged::Ingored);
                }

                // imposta i files aggiunti al commit
                *files_staged = modified_files_vec
                    .clone()
                    .iter()
                    .map(|f| f.to_string())
                    .collect();

                info!(target: "lib", "adding files: {:?}", &modified_files_vec);

                modified_files_vec.insert(0, "add");

                match git(&modified_files_vec, Some(project_path.as_path())) {
                    Ok(_) => {
                        info!(target: "lib", "files added!");
                    }
                    Err(err) => {
                        error!(target: "lib", "{err}");
                        error::<Okay>(
                            &format!("Could not commit the changes, check your internet connection or the selected project path\n{err}"))
                            .show()
                            .unwrap();

                        return Err(err);
                    }
                }

                return Ok(ChangesStaged::Staged);
            }
            Err(err) => {
                error!(target: "lib", "{err}");
                error::<Okay>(
                    &format!("Could not execute the remote repository status, check your internet connection or the selected project path\n{err}"))
                    .show()
                    .unwrap();

                return Err(err);
            }
        }
    }
    Ok(ChangesStaged::Ingored)
}

#[allow(unused)]
pub fn lib_make_push(project_path: &Option<PathBuf>, commit_message: &String) -> Result<(), Error> {
    if let Some(project_path) = project_path {
        // commit
        match git(
            &["commit", "-m", commit_message.as_str()],
            Some(project_path.as_path()),
        ) {
            Ok(_) => {
                info!(target: "lib", "committed with message: '{commit_message}'");
            }
            Err(err) => {
                error!(target: "lib", "{err}");
                error::<Okay>(
                    &format!("Could not commit changes, check your internet connection or the selected project path\n{err}"))
                    .show()
                    .unwrap();

                return Err(err);
            }
        }

        // push
        match git(&["push"], Some(project_path.as_path())) {
            Ok(_) => {
                info!(target: "lib", "pushed with success!");
                return Ok(());
            }
            Err(err) => {
                error!(target: "lib", "{err}");
                error::<Okay>(
                    &format!("Could not complete the push, check your internet connection or the selected project path\n{err}"))
                    .show()
                    .unwrap();
                return Err(err);
            }
        }
    } else {
        error!(target: "lib", "project path not selected");
        Err(Error::Io(io::Error::new(
            io::ErrorKind::NotFound,
            "path file not set!",
        )))
    }
}

#[allow(unused)]
pub fn lib_get_untracked_files(
    project_path: &Option<PathBuf>,
    untracked_files_vec: &mut Vec<(bool, String)>,
) -> Result<(), Error> {
    info!(target: "lib", "getting untracked files");

    if let Some(project_path) = project_path {
        match git(&["status", "-s"], Some(project_path.as_path())) {
            Ok(out) => {
                let outputstr: String = out.stdout.iter().map(|c| *c as char).collect::<String>();
                /*
                Format (outputstr.split_whitespace):
                [src\lib.rs:274:17] files_vec = [
                    "M",
                    "auto-git/src/lib.rs",
                    "??",
                    "auto-git/Cargo.lock",
                ]
                */
                /*
                Alternativa ancora più efficiente: tuple_windows (con itertools)
                Se vuoi evitare del tutto di creare il primo Vec intermedio e lavorare in modo puramente
                "lazy" (pigro), potresti usare la libreria itertools, ma restando sulle funzioni standard
                di Rust, la soluzione sotto è la più leggibile.
                */
                *untracked_files_vec = outputstr
                    .split_terminator("\n")
                    .filter(|f| f.contains("?? "))
                    .map(|m_file| (false, m_file.split_at(3).1.to_string()))
                    .collect();

                // dbg!(untracked_files_vec);

                Ok(())
            }
            Err(err) => {
                error!(target: "lib", "{err}");
                error::<Okay>(
                    &format!("Could not complete status, check your internet connection or the selected project path\n{err}"))
                    .show()
                    .unwrap();
                Err(err)
            }
        }
    } else {
        error!(target: "lib", "project path not selected");
        Err(Error::Io(io::Error::new(
            io::ErrorKind::NotFound,
            "path file not set!",
        )))
    }
}

#[allow(unused)]
pub fn lib_git_add(
    project_path: &Option<PathBuf>,
    untracked_files_vec: &Vec<(bool, String)>,
) -> Result<(), Error> {
    info!("adding untracked files files");

    if let Some(project_path) = project_path {
        let mut files_to_add = untracked_files_vec
            .iter()
            .filter(|(to_add, _)| *to_add == true)
            .map(|(_, file)| file.as_str())
            .collect::<Vec<&str>>();

        files_to_add.insert(0, "add");

        match git(&files_to_add, Some(project_path.as_path())) {
            Ok(_) => {
                info!("files added to commit");
                Ok(())
            }
            Err(err) => {
                error!(target: "lib", "{err}");
                error::<Okay>(
                    &format!("Could not add files to commit, check your internet connection or the selected project path\n{err}"))
                    .show()
                    .unwrap();
                Err(err)
            }
        }
    } else {
        error!(target: "lib", "project path not selected");
        Err(Error::Io(io::Error::new(
            io::ErrorKind::NotFound,
            "path file not set!",
        )))
    }
}

#[allow(unused)]
pub fn lib_get_files_to_restore(
    project_path: &Option<PathBuf>,
    restore_files_vec: &mut Vec<(bool, String, bool)>,
) -> Result<(), Error> {
    info!(target: "lib", "getting modified files to restore");

    // evita i duplicati
    restore_files_vec.clear();

    if let Some(project_path) = project_path {
        match git(&["status", "-s"], Some(project_path.as_path())) {
            Ok(out) => {
                let outputstr: String = out.stdout.iter().map(|c| *c as char).collect::<String>();

                // if staged
                restore_files_vec.extend(
                    outputstr
                        .split_terminator("\n")
                        .filter(|f| !f.contains(" M") && f.contains("M ") || f.contains("MM "))
                        .map(|m_file| (false, m_file.split_at(3).1.to_string(), true))
                        .collect::<Vec<(bool, String, bool)>>(),
                );

                // if not staged
                restore_files_vec.extend(
                    outputstr
                        .split_terminator("\n")
                        .filter(|f| f.contains(" M "))
                        .map(|m_file| (false, m_file.split_at(3).1.to_string(), false))
                        .collect::<Vec<(bool, String, bool)>>(),
                );

                Ok(())
            }
            Err(err) => {
                error!(target: "lib", "{err}");
                error::<Okay>(
                    &format!("Could not complete status, check your internet connection or the selected project path\n{err}"))
                    .show()
                    .unwrap();
                Err(err)
            }
        }
    } else {
        error!(target: "lib", "project path not selected");
        Err(Error::Io(io::Error::new(
            io::ErrorKind::NotFound,
            "path file not set!",
        )))
    }
}

#[allow(unused)]
pub fn lib_git_restore(
    project_path: &Option<PathBuf>,
    restore_files_vec: &Vec<(bool, String, bool)>,
) -> Result<(), Error> {
    info!("adding untracked files files");

    if let Some(project_path) = project_path {
        let mut files_staged_to_restore = restore_files_vec
            .iter()
            .filter(|(to_restore, _, staged)| *to_restore == true && *staged == true)
            .map(|(_, file, _)| file.as_str())
            .collect::<Vec<&str>>();

        if !files_staged_to_restore.is_empty() {
            files_staged_to_restore.insert(0, "restore");
            files_staged_to_restore.insert(1, "--staged");

            match git(&files_staged_to_restore, Some(project_path.as_path())) {
                Ok(_) => {
                    info!("files staged restored");
                    // Ok(())
                }
                Err(err) => {
                    error!(target: "lib", "{err}");
                    error::<Okay>(
                        &format!("Could not restore staged files, check your internet connection or the selected project path\n{err}"))
                        .show()
                        .unwrap();
                    return Err(err);
                    // Err(err)
                }
            }
        }

        let mut files_to_restore = restore_files_vec
            .iter()
            .filter(|(to_restore, _, staged)| *to_restore == true && *staged == false)
            .map(|(_, file, _)| file.as_str())
            .collect::<Vec<&str>>();

        if !files_to_restore.is_empty() {
            files_to_restore.insert(0, "restore");

            match git(&files_to_restore, Some(project_path.as_path())) {
                Ok(_) => {
                    info!("files restored");
                    Ok(())
                }
                Err(err) => {
                    error!(target: "lib", "{err}");
                    error::<Okay>(
                        &format!("Could not restore staged files, check your internet connection or the selected project path\n{err}"))
                        .show()
                        .unwrap();
                    Err(err)
                }
            }
        } else {
            Ok(())
        }
    } else {
        error!(target: "lib", "project path not selected");
        Err(Error::Io(io::Error::new(
            io::ErrorKind::NotFound,
            "path file not set!",
        )))
    }
}

#[cfg(test)]
mod security {
    use super::*;

    #[test]
    fn security_test_1() {
        // std::process::Command::new("chrome.exe")
        //     .arg("C:\\Users\\david\\SIMONE\\INFORMATICA\\Project-Metamorphosis\\project\\auto-git\\hack.html")
        //     .spawn()
        //     .unwrap();
    }
}
