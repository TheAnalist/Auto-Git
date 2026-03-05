use win_msgbox::{error, Okay};
use rfd::FileDialog;
use powershell_script::{PsScript, PsScriptBuilder};
use std::{fmt,fmt::Write, fs, net::TcpStream, path::{Path, PathBuf}, process, io};
pub use log::*;

pub const MAX_TERMINAL_LENGHT :usize = 1000;

const GIT_PROJECT_PATH    :&str = ".git-project"; 
const STARTUP_DIR         :&str = "($env:APPDATA + '\\Auto-Git\\')";

// ---------------------------------- GIT_COMMANDS ----------------------------------
//                          Autor: https://github.com/Timmmm
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


fn git(args: &[&str], working_dir: Option<&Path>) -> Result<process::Output, Error> {
    let mut command = process::Command::new("git");
    if let Some(working_dir) = working_dir {
        command.current_dir(working_dir);
    }

    let output = command
        .args(args)
        .output()?;

    if !output.status.success() {
        return Err(Error::Process(ProcessError {
            output,
            command: std::iter::once(&"git").chain(args.iter()).map(|&s| s.to_owned()).collect(),
        }));
    }

    Ok(output)
}

// ---------------------------------- GIT_COMMANDS ----------------------------------

pub enum ChangesStaged {
    Staged,
    Ingored
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

pub fn lib_check_internet() -> bool {
    info!(target: "lib", "checking internet connection");

    // handle powershell error
    match TcpStream::connect("8.8.8.8:53") {
        Ok(_) => {
            info!(target: "lib", "connected to internet!");
            true
        },
        Err(err) => {
            warn!(target: "lib","unable to connect to internet: {err}");
            false
        }
    }
}

pub fn lib_get_git_project_file_path() -> Option<String> {
    let ps: PsScript = lib_pwsh_psscript_builder();
    let mut path: String = String::with_capacity(STARTUP_DIR.len() + GIT_PROJECT_PATH.len());


    match ps.run(STARTUP_DIR) {
        Ok(output) => {
           write!(path, "{}{GIT_PROJECT_PATH}", output.stdout().unwrap().trim_end().to_string()).unwrap();
           Some(path)
        },
        Err(err) => {
            error::<Okay>(&err.to_string()).show().unwrap();
            error!(target: "lib", "{}", err);
            None
        }
    }
}
// #[inline]
pub fn lib_get_project_path() -> Option<PathBuf> {
    info!(target: "lib", "getting git project path");

    let mut path: String = String::with_capacity(STARTUP_DIR.len() + GIT_PROJECT_PATH.len());
    // let ps: PsScript = lib_pwsh_psscript_builder();
    
    // handle powershell error
    match lib_get_git_project_file_path() {
        Some(output) => {
            path = output
        },
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
        },
        Err(err) => {
            error!(target: "lib", "{err}");
            info!(target: "lib", "creating .git_project file");
            fs::File::create(&path).unwrap();
            None
        }
    }
}

/// aggigorna il progetto locale utilizzando "git remote update" e ritorna lo status
pub fn lib_git_update_local(project_path: &Option<PathBuf>) -> Option<String> {
    // Handle PathBuf None error
    if let Some(pb) = project_path  {
        // remote update
        info!(target: "lib", "remote update");
        match git(&["remote", "update"], Some(pb.as_path())) {
            Ok(_) => {},
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
    } 
    else {
        error::<Okay>("Project path not selected").show().unwrap();
        error!(target: "lib", "project path not selected");
        None
    }
}

pub fn lib_git_status(project_path: &Option<PathBuf>) -> Option<String> {
    if let Some(pb) = project_path {
        info!(target: "lib", "getting project status");
        match git(&["status"], Some(pb.as_path())) {
            Ok(out) => return Some(out.stdout.iter().map(|c| *c as char).collect::<String>()),
            Err(err) => {
                error::<Okay>(&err.to_string()).show().unwrap();
                error!(target: "lib", "{}", err);
                return None
            }
        }
    }

    None
}

pub fn lib_check_remote_ahead(status_string: String) -> bool {
    info!(target: "lib", "check remote ahead");

    if !status_string.contains("Your branch is up to date with") && !status_string.contains("Your branch is ahead of") {
        warn!(target: "lib", "remote project ahead");
        true
    } else {
        info!(target: "lib", "project aligned");
        false
    }
}

pub fn lib_make_pull(project_path: &Option<PathBuf>) -> Result<(), Error> {
    info!(target: "lib", "pulling from remote repository");

    // sono sicuro che project_path sia Some
    let path = project_path.as_ref().unwrap().display().to_string();

    match git(&["pull"], Some(path.as_ref())) {
        Ok(_) => {
            info!(target: "lib", "pull completed with success");
            Ok(())
        },
        Err(err) => {
            error!(target: "lib", "{err}");
            error::<Okay>(
                &format!("Could not pull from the remote repository, check your internet connection or the selected project path\n{err}"))
                .show()
                .unwrap();
            Err(err)
        }
    }
    
}

pub fn lib_stage_changes(project_path: &Option<PathBuf>, files_staged: &mut Vec<String>) -> Result<ChangesStaged, Error>{
    info!(target: "lib","staging changes for commit");

    if let Some(project_path) = project_path {
        match git(&["status", "-s"], Some(project_path.as_path())) {
            Ok(out) => {
                let outputstr :String = out.stdout.iter().map(|c| *c as char).collect::<String>();
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
                    .split_whitespace()
                    .collect::<Vec<&str>>()
                    .windows(2)
                    .filter(|modified_char| modified_char[0] == "M" || modified_char[0] == "MM")
                    .map(|file| file[1])
                    .collect();
                
                // dbg!(&files_staged);
                // dbg!(&modified_files_vec);

                // evita di fare il git add se i files sono uguali
                if modified_files_vec.iter().all(|file | files_staged.contains(&file.to_string())) {
                    // files_staged.clear();
                    return Ok(ChangesStaged::Ingored)
                }

                // imposta i files aggiunti al commit
                *files_staged = modified_files_vec.clone().iter().map(|f| f.to_string()).collect();

                info!(target: "lib", "adding files: {:?}", &modified_files_vec);

                modified_files_vec.insert(0, "add");

                match git(&modified_files_vec, Some(project_path.as_path())) {
                    Ok(_) => {
                        info!(target: "lib", "files added!");
                    },
                    Err(err) => {
                        error!(target: "lib", "{err}");
                        error::<Okay>(
                            &format!("Could not add files to the commit\n{err}"))
                            .show()
                            .unwrap();

                        return Err(err)
                    }
                }

                return Ok(ChangesStaged::Staged)
            },
            Err(err) => {
                error!(target: "lib", "{err}");
                error::<Okay>(
                    &format!("Could not execute the remote repository status, check your internet connection or the selected project path\n{err}"))
                    .show()
                    .unwrap();

                return Err(err)
            }
        }
    }
    Ok(ChangesStaged::Ingored)
}

pub fn lib_make_push(project_path: &Option<PathBuf>, commit_message: &String) -> Result<(), Error> {
    if let Some(project_path) = project_path {
        // commit
        match git(&["commit", "-m", commit_message.as_str()], Some(project_path.as_path())) {
            Ok(_) => {
                info!(target: "lib", "committed with message: '{commit_message}'");
            },
            Err(err) => {
                error!(target: "lib", "{err}");
                error::<Okay>(
                    &format!("Could not commit the changes, check your internet connection or the selected project path\n{err}"))
                    .show()
                    .unwrap();
                
                return Err(err)
            }
        }

        // push
        match git(&["push"], Some(project_path.as_path())) {
            Ok(_) => {
                info!(target: "lib", "pushed with success!");
                return Ok(())
            },
            Err(err) => {
                error!(target: "lib", "{err}");
                error::<Okay>(
                    &format!("Could not complete the push, check your internet connection or the selected project path\n{err}"))
                    .show()
                    .unwrap();
                return Err(err)
            }
        }
    } else {
        error!(target: "lib", "project path not selected");
        Err(Error::Io(io::Error::new(io::ErrorKind::NotFound, "path file not set!")))
    }
}