/*
########################################  Auto-Git Tool  ########################################

                                    Una UI di git per windows.

Dev: The Analist

*/
mod ui;
mod lib;

use ui::gui_app;
use env_logger::Env;

#[allow(unused)]
fn main() {
    // The `Env` lets us tweak what the environment
    // variables to read are and what the default
    // value is if they're missing
    if cfg!(debug_assertions) {
        let env = Env::default()
            .filter_or("AUTOGIT_LOG", "info")
            .write_style_or("AUTOGIT_LOG_STYLE", "always");
    
        env_logger::init_from_env(env);
    }

    gui_app();
}
