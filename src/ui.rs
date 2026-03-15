//Auto-Git UI
use auto_git::*;
use core::f32;
use eframe::egui::{self, Color32, RichText};
use rfd::FileDialog;
use std::{
    fs::{self},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    thread::{self, sleep},
    time::Duration,
};
// use log::*;
// use crate::lib::*;

const OK_TICK: &str = "\u{2714}";
const ERROR_TICK: &str = "❌";

const BUTTON_HEIGHT: f32 = 50.0;
// const MAX_TERMINAL_LENGHT :usize = 1000;

/// Stato dell'applicazione
#[derive(Clone, Debug)]
enum AppState {
    Idle,
    Processing(String),
    Success(String),
    Error(String),
    Searching,
    Exit,
}

/// Dati condivisi tra il thread principale e il thread di allineamento del progetto
pub struct SharedAppData {
    state: Arc<Mutex<AppState>>,
    condvar: Arc<Condvar>,
    project_path: Arc<Mutex<Option<PathBuf>>>,
    status_output: Arc<Mutex<Option<String>>>,
    /// Output del terminale (log delle operazioni)
    terminal_output: Arc<Mutex<Vec<String>>>,
    files_staged: Arc<Mutex<Vec<String>>>,
}

/// Struttura principale dell'applicazione
pub struct AutoGitApp {
    /// Stato corrente dell'applicazione
    shared_data: SharedAppData,
    /// Indica se il pannello terminale è aperto
    terminal_expanded: bool,
    /// Altezza del pannello terminale quando espanso
    terminal_height: f32,
    commit_input_expanded: bool,
    commit_message: String,
    complete_push: bool,
    advanced_git_options: bool,
    untracked_files: Vec<(bool, String)>,
    untracked_files_expanded: bool,
    untracked_files_to_add: bool,
    //                        (0: to_restore, 1: path, 2: staged)
    restore_files: Vec<(bool, String, bool)>,
    restore_files_expanded: bool,
    files_to_restore: bool,
}

fn back_align_project_n_stage_changes(shared_data: SharedAppData, repaint: egui::Context) {
    info!("starting checking thread");
    thread::spawn(move || {
        sleep(Duration::from_secs(1));
        const POLL_INTERVAL: Duration = Duration::from_secs(25);

        loop {
            {
                let mut status = shared_data.state.lock().unwrap();

                while matches!(*status, AppState::Processing(_))
                    || matches!(*status, AppState::Error(_))
                {
                    status = shared_data.condvar.wait(status).unwrap();
                }
                // info!("searching");
                *status = AppState::Searching;

                let mut files_staged = shared_data.files_staged.lock().unwrap();

                // --------------------------- Stage files per il push ---------------------------
                if files_staged.is_empty() {
                    shared_data
                        .terminal_output
                        .lock()
                        .unwrap()
                        .push("▶ Adding modified files to commit...".to_string());
                }

                // stage modified files for commit
                match lib_stage_changes(
                    &shared_data.project_path.lock().unwrap(),
                    &mut files_staged,
                ) {
                    Ok(changes_staged) => {
                        if matches!(changes_staged, ChangesStaged::Staged) {
                            *status = AppState::Success(
                                "Files successfully added to the commit".to_string(),
                            );
                            shared_data.terminal_output.lock().unwrap().push(format!(
                                "{OK_TICK} Files successfully added to the commit! ({})",
                                files_staged.join(", ")
                            ));
                        }
                    }
                    Err(_) => {
                        *status =
                            AppState::Error("Error during file staging execution".to_string());
                        shared_data
                            .terminal_output
                            .lock()
                            .unwrap()
                            .push(format!("{ERROR_TICK} Error during file staging execution"));
                    }
                }
                // -------------------------------------------------------------------------------
            }
            repaint.request_repaint();
            {
                let status = shared_data.state.lock().unwrap();

                if matches!(*status, AppState::Exit) {
                    break;
                }

                if !lib_check_internet() {
                    // sleep(POLL_INTERVAL);
                    let _ = shared_data
                        .condvar
                        .wait_timeout(status, POLL_INTERVAL)
                        .unwrap();
                    continue;
                }
            }

            repaint.request_repaint();

            {
                let mut status = shared_data.state.lock().unwrap();

                // status
                *shared_data.status_output.lock().unwrap() =
                    lib_git_update_local(&shared_data.project_path.lock().unwrap());

                // check remote ahead and pull
                if let Some(out) = shared_data.status_output.lock().unwrap().clone() {
                    if lib_check_remote_ahead(out) {
                        match lib_make_pull(
                            &shared_data.project_path.lock().unwrap(),
                            &mut shared_data.terminal_output.lock().unwrap(),
                        ) {
                            Ok(_) => {
                                *status =
                                    AppState::Success("Pull completed successfully".to_string());

                                // if let Ok(mut terminal_output) = shared_data.terminal_output.try_lock() {
                                //     terminal_output.push(format!("{OK_TICK} Pull completato con successo!"));
                                // }
                            }
                            Err(_) => {
                                *status = AppState::Error("Error in pull operation".to_string());

                                // if let Ok(mut terminal_output) = shared_data.terminal_output.try_lock() {
                                //     terminal_output.push(format!("{ERROR_TICK} Errore nell'operzione di pull"));
                                // }
                            }
                        }
                    }
                } else {
                    if shared_data.project_path.lock().unwrap().is_none() {
                        *status = AppState::Error(
                            "Configure the project path from the menu 'Set Project'".to_string(),
                        );
                        shared_data.terminal_output.lock().unwrap().push(format!(
                            "{ERROR_TICK} Error executing the status command: Set the project path"
                        ));
                    } else {
                        *status = AppState::Error("Status incomplete".to_string());
                        shared_data
                            .terminal_output
                            .lock()
                            .unwrap()
                            .push(format!("{ERROR_TICK} Error executing the status command"));
                    }
                    continue;
                }

                // se dopo le operazioni
                if matches!(*status, AppState::Processing(_)) {
                    continue;
                }
            }

            repaint.request_repaint();

            {
                let status = shared_data.state.lock().unwrap();

                if matches!(*status, AppState::Processing(_)) {
                    continue;
                }

                // sleep(POLL_INTERVAL);
                let _ = shared_data
                    .condvar
                    .wait_timeout(status, POLL_INTERVAL)
                    .unwrap();
            }
        }
    });
}

impl AutoGitApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let state = Arc::new(Mutex::new(AppState::Idle));
        let condvar = Arc::new(Condvar::new());
        let project_path = Arc::new(Mutex::new(lib_get_project_path()));
        let status_output = Arc::new(Mutex::new(None));
        let terminal_output = Arc::new(Mutex::new(Vec::new()));
        let files_staged = Arc::new(Mutex::new(Vec::new()));

        back_align_project_n_stage_changes(
            SharedAppData {
                state: Arc::clone(&state),
                condvar: Arc::clone(&condvar),
                project_path: Arc::clone(&project_path),
                status_output: Arc::clone(&status_output),
                terminal_output: Arc::clone(&terminal_output),
                files_staged: Arc::clone(&files_staged),
            },
            cc.egui_ctx.clone(),
        );

        Self {
            shared_data: SharedAppData {
                state,
                condvar,
                project_path,
                status_output,
                terminal_output,
                files_staged,
            },
            terminal_expanded: false,
            terminal_height: 200.0,
            commit_input_expanded: false,
            commit_message: String::new(),
            complete_push: false,
            advanced_git_options: false,
            untracked_files: Vec::new(),
            untracked_files_expanded: false,
            untracked_files_to_add: false,
            restore_files: Vec::new(),
            restore_files_expanded: false,
            files_to_restore: false,
        }
    }

    fn begin_operation(&mut self, name: &str) {
        *self.shared_data.state.lock().unwrap() = AppState::Processing(name.to_string());
        self.add_terminal_output(format!("▶ Starting operation {name}..."));
        // self.shared_data.condvar.notify_all(); // il worker esce dal wait_timeout e va al punto 1

        // repaint.request_repaint();
    }

    /// Aggiunge una linea di output al terminale
    pub fn add_terminal_output(&mut self, line: String) {
        let mut output = self.shared_data.terminal_output.lock().unwrap();
        output.push(line);
        // Mantieni solo le ultime 1000 righe
        if output.len() > MAX_TERMINAL_LENGHT {
            output.remove(0);
        }
    }

    /// Non fa nulla perchè interrompe l'esecuzione della funzione precedente
    fn idle(&self) {
        info!("ui idle");
        self.shared_data.condvar.notify_all();
    }

    /// Operazione di Push
    fn handle_push(&mut self) {
        info!("handling status");
        self.begin_operation("Push");

        self.commit_input_expanded = true;
        self.add_terminal_output("\u{2714} Starting Push operation".to_string());

        if self.complete_push {
            let mut in_error_state: bool = false;

            match lib_make_push(
                &self.shared_data.project_path.lock().unwrap(),
                &self.commit_message,
            ) {
                Ok(_) => {
                    if let Ok(mut state) = self.shared_data.state.try_lock() {
                        in_error_state = matches!(*state, AppState::Error(_));
                        *state = AppState::Success("Push completed successfully".to_string());
                    }

                    self.shared_data
                        .terminal_output
                        .lock()
                        .unwrap()
                        .push(format!("{OK_TICK} Push completed successfully"));

                    self.shared_data.files_staged.lock().unwrap().clear();

                    if in_error_state {
                        self.shared_data.condvar.notify_all();
                    }
                }
                Err(err) => match err {
                    Error::Io(_) => {
                        *self.shared_data.state.lock().unwrap() =
                            AppState::Error("Project path not selected".to_string());
                        self.shared_data.terminal_output.lock().unwrap().push(format!("{ERROR_TICK} Configure the project path from the menu 'Set Project'"));
                    }
                    _ => {
                        *self.shared_data.state.lock().unwrap() =
                            AppState::Error("Operation incomplete due to an error".to_string());
                        self.shared_data
                            .terminal_output
                            .lock()
                            .unwrap()
                            .push(format!(
                                "{ERROR_TICK} Operation incomplete due to an error: {}",
                                err
                            ));
                    }
                },
            }

            self.commit_input_expanded = false;
            self.complete_push = false;
        }
    }

    /// Operazione di Ignora
    fn handle_ignore(&mut self) {
        info!("handling ignore");
        let mut in_error_state: bool = false;

        self.begin_operation("Ignore");
        sleep(Duration::from_millis(50));
        self.shared_data.files_staged.lock().unwrap().clear();

        self.add_terminal_output("\u{2714} Project changes discarded".to_string());

        if let Ok(mut state) = self.shared_data.state.try_lock() {
            in_error_state = matches!(*state, AppState::Error(_));
            *state = AppState::Success("Changes ignored successfully".to_string());
            // self.idle();
        }

        if in_error_state {
            self.shared_data.condvar.notify_all();
        }
    }

    /// Operazione di Status
    fn handle_status(&mut self) {
        info!("handling status");
        self.begin_operation("Status");

        // se non c'è connessione a internet
        {
            let mut status_output = self.shared_data.status_output.lock().unwrap();
            if status_output.is_none() {
                *status_output = lib_git_status(&self.shared_data.project_path.lock().unwrap());
            }
        }

        let out = Arc::clone(&self.shared_data.status_output);

        match out.lock().unwrap().clone() {
            Some(output) => {
                let mut in_error_state: bool = false;

                self.add_terminal_output(output);
                // apre il terminale per mostrare l'output
                self.terminal_expanded = true;

                if let Ok(mut state) = self.shared_data.state.try_lock() {
                    in_error_state = matches!(*state, AppState::Error(_));
                    *state = AppState::Success("Status check completed successfully".to_string());
                }

                self.add_terminal_output("\u{2714} Repository status verified!".to_string());

                if in_error_state {
                    self.shared_data.condvar.notify_all();
                }
            }
            None => {
                self.add_terminal_output(
                    "Error: Could not perform the status operation".to_string(),
                );
                self.terminal_expanded = true;
                *self.shared_data.state.lock().unwrap() =
                    AppState::Error("Error in status operation execution".to_string());
            }
        }
    }

    /// Operazione di Add dei files
    fn handle_add(&mut self) {
        info!("handling add");
        self.begin_operation("Add");

        // show untracked files (ui checkbox)
        if !self.untracked_files_expanded && !self.untracked_files_to_add {
            match lib_get_untracked_files(
                &self.shared_data.project_path.lock().unwrap().clone(),
                &mut self.untracked_files,
            ) {
                Ok(_) => {
                    self.shared_data
                        .terminal_output
                        .lock()
                        .unwrap()
                        .push(format!("{OK_TICK} 'Untracked files' found!"));
                    self.untracked_files_expanded = true;
                }
                Err(err) => match err {
                    Error::Io(_) => {
                        *self.shared_data.state.lock().unwrap() =
                            AppState::Error("Project path not set".to_string());
                        self.shared_data.terminal_output.lock().unwrap().push(format!("{ERROR_TICK} Configure the project path from the menu 'Set Project'"));
                    }
                    _ => {
                        *self.shared_data.state.lock().unwrap() =
                            AppState::Error("Add operation incomplete due to an error".to_string());
                        self.shared_data
                            .terminal_output
                            .lock()
                            .unwrap()
                            .push(format!("{ERROR_TICK} Add operation errored: {}", err));
                    }
                },
            }
        }

        if self.untracked_files_to_add {
            match lib_git_add(
                &self.shared_data.project_path.lock().unwrap(),
                &self.untracked_files,
            ) {
                Ok(_) => {
                    self.shared_data
                        .terminal_output
                        .lock()
                        .unwrap()
                        .push(format!(
                            "{OK_TICK} Files {} added to commit!",
                            self.untracked_files
                                .iter()
                                .filter(|(added, _)| added.eq(&true))
                                .count()
                        ));
                    *self.shared_data.state.lock().unwrap() =
                        AppState::Success("Files added to commit successfully".to_string());
                }
                Err(err) => match err {
                    Error::Io(_) => {
                        *self.shared_data.state.lock().unwrap() =
                            AppState::Error("Project path not set".to_string());
                        self.shared_data.terminal_output.lock().unwrap().push(format!("{ERROR_TICK} Configure the project path from the menu 'Set Project'"));
                    }
                    _ => {
                        *self.shared_data.state.lock().unwrap() =
                            AppState::Error("Add operation incomplete due to an error".to_string());
                        self.shared_data
                            .terminal_output
                            .lock()
                            .unwrap()
                            .push(format!("{ERROR_TICK} Add operation errored: {}", err));
                    }
                },
            }
        }
    }

    /// Operazione di Restore dei files
    fn handle_restore(&mut self) {
        info!("handling restore");
        self.begin_operation("Restore");

        if !self.restore_files_expanded && !self.files_to_restore {
            match lib_get_files_to_restore(
                &self.shared_data.project_path.lock().unwrap().clone(),
                &mut self.restore_files,
            ) {
                Ok(_) => {
                    self.shared_data
                        .terminal_output
                        .lock()
                        .unwrap()
                        .push(format!("{OK_TICK} Modified files to restore found!"));
                    self.restore_files_expanded = true;
                }
                Err(err) => match err {
                    Error::Io(_) => {
                        *self.shared_data.state.lock().unwrap() =
                            AppState::Error("Project path not set".to_string());
                        self.shared_data.terminal_output.lock().unwrap().push(format!("{ERROR_TICK} Configure the project path from the menu 'Set Project'"));
                    }
                    _ => {
                        *self.shared_data.state.lock().unwrap() = AppState::Error(
                            "Restore operation incomplete due to an error".to_string(),
                        );
                        self.shared_data
                            .terminal_output
                            .lock()
                            .unwrap()
                            .push(format!("{ERROR_TICK} Restore operation errored: {}", err));
                    }
                },
            }
        }

        if self.files_to_restore {
            match lib_git_restore(
                &self.shared_data.project_path.lock().unwrap(),
                &self.restore_files,
            ) {
                Ok(_) => {
                    self.shared_data
                        .terminal_output
                        .lock()
                        .unwrap()
                        .push(format!(
                            "{OK_TICK} Files {} restored!",
                            self.restore_files
                                .iter()
                                .filter(|(restored, _, _)| restored.eq(&true))
                                .count()
                        ));
                    *self.shared_data.state.lock().unwrap() =
                        AppState::Success("File(s) restored successfully".to_string());
                }
                Err(err) => match err {
                    Error::Io(_) => {
                        *self.shared_data.state.lock().unwrap() =
                            AppState::Error("Project path not set".to_string());
                        self.shared_data.terminal_output.lock().unwrap().push(format!("{ERROR_TICK} Configure the project path from the menu 'Set Project'"));
                    }
                    _ => {
                        *self.shared_data.state.lock().unwrap() = AppState::Error(
                            "Restore operation incomplete due to an error".to_string(),
                        );
                        self.shared_data
                            .terminal_output
                            .lock()
                            .unwrap()
                            .push(format!("{ERROR_TICK} Restore operation errored: {}", err));
                    }
                },
            }
            self.files_to_restore = false;
            self.restore_files_expanded = false;
        }
    }

    /// Operazione di Clone dei files
    fn handle_clone(&mut self) {
        info!("handling clone");
    }

    fn draw_menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(RichText::new("☰ Menu").size(17.0), |ui| {
                if ui.button("Set Project").clicked() {
                    info!("setting new project path");
                    let mut project_path = self.shared_data.project_path.lock().unwrap();

                    if let Some(path) = FileDialog::new()
                        .set_can_create_directories(true)
                        // .set_directory(project_path.clone().unwrap_or(PathBuf::from("C:\\Users\\")))
                        .pick_folder()
                    {
                        *project_path = Some(path.clone());

                        {
                            let mut terminal_output =
                                self.shared_data.terminal_output.lock().unwrap();
                            let mut in_error_state: bool = false;

                            terminal_output
                                .push(format!("{OK_TICK} New project path set {}", path.display()));

                            if let Ok(mut state) = self.shared_data.state.try_lock() {
                                in_error_state = matches!(*state, AppState::Error(_));
                                *state = AppState::Success(format!(
                                    "New project path set {}",
                                    path.display()
                                ));
                            }

                            if in_error_state {
                                self.shared_data.condvar.notify_all();
                            }
                        }

                        self.commit_input_expanded = false;
                        self.untracked_files_expanded = false;
                    } else {
                        {
                            if let Ok(mut state) = self.shared_data.state.try_lock() {
                                *state = AppState::Idle;
                            }
                        }
                    }
                }

                if ui
                    .checkbox(&mut self.advanced_git_options, "Advanced Options")
                    .clicked()
                {
                    info!("advanced options visible: {}", self.advanced_git_options);
                }

                if ui
                    .button("Reset")
                    .highlight()
                    .on_hover_text("Erase errors by resetting the app state")
                    .clicked()
                {
                    info!("resetting state");
                    *self.shared_data.state.lock().unwrap() = AppState::Idle;
                    self.commit_input_expanded = false;
                    self.untracked_files_expanded = false;
                    self.restore_files_expanded = false;
                    self.idle();
                }
            });
        });
    }

    /// Disegna i bottoni principali
    fn draw_buttons(&mut self, ui: &mut egui::Ui) {
        let is_processing = matches!(
            *self.shared_data.state.lock().unwrap(),
            AppState::Processing(_)
        );
        let files_staged_empty = self.shared_data.files_staged.lock().unwrap().is_empty();

        // Calcola la larghezza disponibile e dividi per 3 bottoni + spaziatura
        let available_width = ui.available_width() - 60.0; // 40px per margini, 20px per spacing
        let button_width = available_width / 3.0;

        ui.add_space(20.0);

        ui.horizontal(|ui| {
            ui.add_space(20.0);

            // Bottone Push
            ui.add_enabled_ui(!is_processing && !files_staged_empty, |ui| {
                let push_button =
                    egui::Button::new(egui::RichText::new("🚀 Push").size(18.0).strong())
                        .min_size(egui::vec2(button_width, BUTTON_HEIGHT))
                        .fill(egui::Color32::from_rgb(76, 175, 80)); // Verde

                if ui.add(push_button).clicked() {
                    self.handle_push();
                }
            });

            ui.add_space(10.0);

            // Bottone Ignora
            ui.add_enabled_ui(!is_processing, |ui| {
                let ignore_button =
                    egui::Button::new(egui::RichText::new("⏭ Ignore").size(18.0).strong())
                        .min_size(egui::vec2(button_width, BUTTON_HEIGHT))
                        .fill(egui::Color32::from_rgb(255, 152, 0)); // Arancione

                if ui.add(ignore_button).clicked() {
                    self.handle_ignore();
                }
            });

            ui.add_space(10.0);

            // Bottone Status
            ui.add_enabled_ui(!is_processing, |ui| {
                let status_button =
                    egui::Button::new(egui::RichText::new("📊 Status").size(18.0).strong())
                        .min_size(egui::vec2(button_width, BUTTON_HEIGHT))
                        .fill(egui::Color32::from_rgb(33, 150, 243)); // Blu

                if ui.add(status_button).clicked() {
                    self.handle_status();
                }
            });

            ui.add_space(20.0);
        });

        ui.add_space(10.0);

        if self.advanced_git_options {
            ui.horizontal(|ui| {
                ui.add_space(20.0);

                ui.add_enabled_ui(!is_processing, |ui| {
                    let add_button =
                        egui::Button::new(egui::RichText::new("➕ Add").size(18.0).strong())
                            .min_size(egui::vec2(button_width, BUTTON_HEIGHT))
                            .fill(egui::Color32::from_rgb(76, 130, 80));

                    if ui.add(add_button).clicked() {
                        self.handle_add();
                    }
                });

                ui.add_space(10.0);

                ui.add_enabled_ui(!is_processing, |ui| {
                    let restore_button =
                        egui::Button::new(egui::RichText::new("🔁 Restore").size(18.0).strong())
                            .min_size(egui::vec2(button_width, BUTTON_HEIGHT))
                            .fill(egui::Color32::from_rgb(255, 130, 0));

                    if ui.add(restore_button).clicked() {
                        self.handle_restore();
                    }
                });

                ui.add_space(10.0);

                ui.add_enabled_ui(!is_processing, |ui| {
                    let clone_button =
                        egui::Button::new(egui::RichText::new("📩 Clone").size(18.0).strong())
                            .min_size(egui::vec2(button_width, BUTTON_HEIGHT))
                            .fill(egui::Color32::from_rgb(33, 110, 255)); // Blu

                    if ui.add(clone_button).clicked() {
                        self.handle_clone();
                    }
                });

                ui.add_space(20.0);
            });
        }

        ui.add_space(20.0);
    }

    /// Disegna l'indicatore di stato e lo spinner
    fn draw_status_indicator(&mut self, ui: &mut egui::Ui) {
        let is_processing = matches!(
            *self.shared_data.state.lock().unwrap(),
            AppState::Processing(_)
        );
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            ui.add_space(20.0);

            let state = self.shared_data.state.lock().unwrap().clone();

            match state {
                AppState::Idle => {
                    ui.label(
                        egui::RichText::new("⚪ Waiting for operations...")
                            .size(14.0)
                            .color(egui::Color32::GRAY),
                    );
                }
                AppState::Processing(operation) => {
                    ui.spinner();
                    ui.add_space(5.0);
                    ui.label(
                        egui::RichText::new(format!("⏳ Processing: {}", operation))
                            .size(14.0)
                            .color(egui::Color32::from_rgb(33, 150, 243)),
                    );
                }
                AppState::Success(message) => {
                    ui.label(
                        // egui::RichText::new(format!("✓ {}", message))
                        egui::RichText::new(format!("{OK_TICK} {}", message))
                            .size(14.0)
                            .color(egui::Color32::from_rgb(76, 175, 80)),
                    );
                }
                AppState::Error(message) => {
                    ui.label(
                        // egui::RichText::new(format!("✗ Errore: {}", message))
                        egui::RichText::new(format!("{ERROR_TICK} Error: {}", message))
                            .size(14.0)
                            .color(egui::Color32::from_rgb(244, 67, 54)),
                    );
                }
                AppState::Searching => {
                    ui.spinner();
                    ui.add_space(5.0);
                    ui.label(
                        egui::RichText::new("Checking for changes...")
                            .size(14.0)
                            .color(egui::Color32::from_rgb(33, 150, 243)),
                    );
                }
                _ => {}
            }

            ui.add_space(ui.available_width() - 120.0);

            // abort button
            if is_processing {
                ui.add_enabled_ui(is_processing, |ui| {
                    let abort_button =
                        egui::Button::new(egui::RichText::new("❌ Abort").size(15.0).strong())
                            .min_size(egui::vec2(5.0, 30.0))
                            .fill(egui::Color32::from_rgb(150, 30, 23));

                    if ui.add(abort_button).clicked() {
                        self.idle();
                        self.commit_input_expanded = false;
                        self.untracked_files_expanded = false;
                        self.restore_files_expanded = false;
                        self.add_terminal_output("Operation aborted".to_string());
                        *self.shared_data.state.lock().unwrap() = AppState::Idle;
                    }
                });
            }
        });

        ui.add_space(10.0);
    }

    /// Disegna il pannello del terminale a scomparsa
    fn draw_terminal_panel(&mut self, _ui: &mut egui::Ui, ctx: &egui::Context) {
        // ui.separator();

        // Intestazione del pannello con pulsante per espandere/comprimere
        egui::TopBottomPanel::bottom("Output Teminale Buttons")
            .exact_height(40.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // ui.add_space(10.0);

                    let arrow = if self.terminal_expanded {
                        "🔽"
                    } else {
                        "▶"
                    }; // ▼ e ►
                    let label_text = format!("{} Terminal Output", arrow);
                    // println!("{}", "\u{25BC}");
                    let response = ui.button(egui::RichText::new(label_text).size(15.0).strong());

                    if response.clicked() {
                        self.terminal_expanded = !self.terminal_expanded;
                    }

                    ui.add_space(10.0);

                    // Bottone per pulire l'output
                    if self.terminal_expanded && ui.button("🗑 Clear").clicked() {
                        self.shared_data.terminal_output.lock().unwrap().clear();
                    };

                    // response
                });
            });

        // Pannello espandibile
        if self.terminal_expanded {
            // ui.separator();
            egui::TopBottomPanel::bottom("Output Panel")
                .resizable(true)
                .default_height(self.terminal_height)
                .show(ctx, |ui| {
                    let available_height = ui.available_height();

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(available_height)
                        // .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.add_space(5.0);

                            let output = self.shared_data.terminal_output.lock().unwrap();

                            if output.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.add_space(10.0);
                                    ui.label(
                                        egui::RichText::new("No output avaliable")
                                            .size(12.0)
                                            .color(egui::Color32::GRAY)
                                            .italics(),
                                    );
                                });
                            } else {
                                for line in output.iter() {
                                    ui.horizontal(|ui| {
                                        ui.add_space(10.0);

                                        // Colora diversamente in base al tipo di messaggio
                                        let color = if line.starts_with("\u{2714}") {
                                            // ✓
                                            egui::Color32::from_rgb(76, 175, 80)
                                        } else if line.starts_with(ERROR_TICK)
                                            || line.contains("error")
                                            || line.contains("Error")
                                        {
                                            egui::Color32::from_rgb(244, 67, 54)
                                        } else if line.starts_with("▶") {
                                            egui::Color32::from_rgb(33, 150, 243)
                                        } else {
                                            egui::Color32::LIGHT_GRAY
                                        };

                                        ui.label(
                                            egui::RichText::new(line)
                                                .size(12.0)
                                                .color(color)
                                                .family(egui::FontFamily::Monospace),
                                        );
                                    });
                                }
                            }

                            ui.add_space(5.0);
                        });
                });
        }
    }

    fn draw_commit_input(&mut self, ui: &mut egui::Ui) {
        // let mut binding = "Commit message";

        if self.commit_input_expanded {
            // ui.label(RichText::new("Commit message").size(15.).strong());

            ui.vertical_centered(|ui| {
                ui.set_max_width(ui.available_width() - 60.0);

                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::symmetric(20, 15))
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.heading(RichText::new("📋 Commit message").size(15.).strong())
                        });

                        ui.separator();

                        egui::ScrollArea::vertical().show(ui, |ui| {
                            let editor = egui::TextEdit::multiline(&mut self.commit_message)
                                .font(egui::TextStyle::Monospace) // for cursor height
                                .code_editor()
                                // .desired_rows(10)
                                .lock_focus(true)
                                .desired_width(f32::INFINITY)
                                .background_color(Color32::from_rgb(30, 30, 25));

                            ui.add(editor);
                        });

                        let push_button =
                            egui::Button::new(egui::RichText::new("Push 🚀").size(18.0).strong())
                                .min_size(egui::vec2(200., BUTTON_HEIGHT))
                                .fill(egui::Color32::from_rgb(76, 175, 80));

                        if ui.add(push_button).clicked() {
                            self.add_terminal_output(format!(
                                "▶ Committing elements with message: '{}'",
                                self.commit_message
                            ));
                            self.complete_push = true;
                            self.handle_push();
                        }
                    })
            });
            // ui.vertical_centered(|ui| {

            // });
        }
    }

    fn draw_add_untracked_files(&mut self, ui: &mut egui::Ui) {
        if self.untracked_files_expanded {
            ui.vertical_centered(|ui| {
                ui.set_max_width(ui.available_width() - 60.0);

                let num_col =
                    (ui.available_width() - 60.0) / (10.0 * self.untracked_files.len() as f32);
                // println!("{num_col}");

                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::symmetric(20, 15))
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.heading(RichText::new("📂 Add files").size(15.).strong())
                        });

                        ui.separator();

                        egui::Grid::new("files_grid")
                            .num_columns(3)
                            .spacing([20.0, 15.0])
                            .striped(true)
                            .show(ui, |ui| {
                                for (index, untracked_file) in
                                    self.untracked_files.iter_mut().enumerate()
                                {
                                    ui.checkbox(&mut untracked_file.0, untracked_file.1.clone());

                                    if (index + 1) % num_col.round().floor() as usize == 0 {
                                        ui.end_row();
                                    }
                                }
                            });

                        ui.add_space(30.0);

                        let all_button =
                            egui::Button::new(egui::RichText::new("All").size(13.0).strong())
                                .min_size(egui::vec2(30., 20.))
                                .fill(egui::Color32::from_rgb(90, 110, 100));

                        let add_button = egui::Button::new(
                            egui::RichText::new("Add").size(13.0).strong().heading(),
                        )
                        .min_size(egui::vec2(30., 20.))
                        .fill(egui::Color32::from_rgb(76, 190, 80));

                        ui.horizontal(|ui| {
                            ui.add_space(ui.available_width() / 2.0 - 45.0);
                            if ui.add(all_button).clicked() {
                                self.untracked_files
                                    .iter_mut()
                                    .for_each(|(to_add, _)| *to_add = true);
                            }

                            ui.add_space(15.0);

                            if ui.add(add_button).clicked() {
                                self.untracked_files_to_add = true;
                                self.untracked_files_expanded = false;
                                self.handle_add();
                            }
                        });
                    });
            });
        }
    }

    fn draw_restore_files(&mut self, ui: &mut egui::Ui) {
        if self.restore_files_expanded {
            ui.vertical_centered(|ui| {
                ui.set_max_width(ui.available_width() - 100.0);

                let num_col =
                    (ui.available_width() - 100.0) / (10.0 * self.restore_files.len() as f32);
                // println!("{num_col}");

                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::symmetric(20, 15))
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.heading(RichText::new("⏮ Restore files").size(15.).strong())
                        });

                        ui.separator();

                        egui::Grid::new("files_grid")
                            .num_columns(3)
                            .spacing([20.0, 15.0])
                            .striped(true)
                            .show(ui, |ui| {
                                for (index, restore_file) in
                                    self.restore_files.iter_mut().enumerate()
                                {
                                    ui.checkbox(&mut restore_file.0, restore_file.1.clone());

                                    if (index + 1) % num_col.round().floor() as usize == 0 {
                                        ui.end_row();
                                    }
                                }
                            });

                        ui.add_space(30.0);

                        let all_button =
                            egui::Button::new(egui::RichText::new("All").size(13.0).strong())
                                .min_size(egui::vec2(30., 20.))
                                .fill(egui::Color32::from_rgb(90, 110, 100));

                        let restore_button = egui::Button::new(
                            egui::RichText::new("Restore").size(13.0).strong().heading(),
                        )
                        .min_size(egui::vec2(30., 20.))
                        .fill(egui::Color32::from_rgb(76, 190, 80));

                        ui.horizontal(|ui| {
                            ui.add_space(ui.available_width() / 2.0 - 70.0);
                            if ui.add(all_button).clicked() {
                                self.restore_files
                                    .iter_mut()
                                    .for_each(|(to_restore, _, _)| *to_restore = true);
                            }

                            ui.add_space(15.0);

                            if ui.add(restore_button).clicked() {
                                self.files_to_restore = true;
                                self.restore_files_expanded = false;
                                self.handle_restore();
                            }
                        });
                    });
            });
        }
    }
}

impl eframe::App for AutoGitApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // verifica modifiche al progetto remoto e prepara le modifiche per il push
        // self.align_project_n_stage_changes();
        // Configura lo stile generale
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(45, 45, 45);
        style.visuals.extreme_bg_color = egui::Color32::from_rgb(30, 30, 30);
        ctx.set_style(style);

        egui::CentralPanel::default().show(ctx, |ui| {
            // let available_height = ui.available_height();
            self.draw_menu(ui);

            ctx.request_repaint();

            // Header
            ui.vertical_centered(|ui| {
                ui.add_space(15.0);
                ui.heading(egui::RichText::new("🔄 Auto-Git").size(26.0).strong());
                ui.add_space(5.0);
                ui.label(
                    egui::RichText::new("Automatic Git repository synchronization")
                        .size(13.0)
                        .color(egui::Color32::GRAY),
                );
            });

            ui.add_space(10.0);
            ui.separator();

            // Bottoni principali
            self.draw_buttons(ui);

            ui.separator();

            // Indicatore di stato
            self.draw_status_indicator(ui);

            self.draw_commit_input(ui);
            self.draw_add_untracked_files(ui);
            self.draw_restore_files(ui);

            // Spazio flessibile per spingere il pannello terminale in basso
            // ui.add_space(available_height - 300.0);
            // Pannello terminale (sempre in basso)
            self.draw_terminal_panel(ui, ctx);
        });

        // Richiedi un ridisegno continuo quando c'è un'operazione in corso
        if matches!(
            *self.shared_data.state.lock().unwrap(),
            AppState::Processing(_)
        ) {
            ctx.request_repaint();
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        info!("exiting Auto-Git");
        // in uscita scrive il percorso usato dall'applicazione nel file .git-project
        if let Some(ref pp) = *self.shared_data.project_path.lock().unwrap() {
            info!("writing new path in .git-project file: {}", pp.display());
            fs::write(
                lib_get_git_project_file_path().unwrap(),
                pp.display().to_string(),
            )
            .unwrap();
        }
        *self.shared_data.state.lock().unwrap() = AppState::Exit;

        self.shared_data.condvar.notify_all();
    }
}

pub(crate) fn gui_app() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 750.0])
            .with_min_inner_size([600.0, 550.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(&include_bytes!("../icon.png")[..])
                    .unwrap_or_default(),
            ),
        ..Default::default()
    };

    eframe::run_native(
        "Auto-Git",
        options,
        Box::new(|cc| Ok(Box::new(AutoGitApp::new(cc)))),
    )
}
