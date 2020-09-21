// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::env;
use std::process::{exit, Command};
use std::thread::sleep;
use std::time::Duration;

use structopt::StructOpt;

/// Mimic the behaviour of `firefox.exe` on Windows.
///
/// By default, the first run of firefox.exe starts the "Launcher Process", which
/// does a bunch of work before re-executing `firefox.exe` as the main (parent)
/// process.

/// Matches the options passed to `firefox.exe` by `fxrunner.
#[derive(StructOpt)]
struct LauncherOptions {
    #[structopt(long = "profile")]
    _profile: String,

    #[structopt(long)]
    new_instance: bool,

    #[structopt(long)]
    wait_for_browser: bool,
}

/// Options to distinguish main process mode from launcher mode.
#[derive(StructOpt)]
struct MainOptions {
    #[structopt(long)]
    main: bool,
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if let Ok(opts) = LauncherOptions::from_iter_safe(&args) {
        eprintln!("[launcher] args: {:?}", args);
        // Launcher process.
        // Launch this executable with different command
        assert!(opts.wait_for_browser);
        assert!(opts.new_instance);

        let mut child = Command::new(&args[0])
            .arg("--main")
            .spawn()
            .expect("Could not spawn child");

        eprintln!("[launcher] spawned child process {}", child.id());

        let exit_status = child.wait().expect("wait()");
        let code = exit_status.code().unwrap();

        eprintln!("[launcher] child exited: {}", code);

        exit(code);
    } else if let Ok(opts) = MainOptions::from_iter_safe(&args) {
        if opts.main {
            eprintln!("[main] args: {:?}", args);
            assert!(opts.main);

            // Main process.
            // Just spin the even loop waiting to be terminated.
            loop {
                sleep(Duration::from_secs(30));
            }
        }
    }

    eprintln!("[fakefox] args: {:?}", args);
    panic!("[fakefox] not executed as child or parent");
}
