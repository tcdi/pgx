//LICENSE Portions Copyright 2019-2021 ZomboDB, LLC.
//LICENSE
//LICENSE Portions Copyright 2021-2023 Technology Concepts & Design, Inc.
//LICENSE
//LICENSE Portions Copyright 2023-2023 PgCentral Foundation, Inc. <contact@pgcentral.org>
//LICENSE
//LICENSE All rights reserved.
//LICENSE
//LICENSE Use of this source code is governed by the MIT license that can be found in the LICENSE file.
use std::collections::HashSet;
use std::process::{Command, Stdio};

use eyre::{eyre, WrapErr};
use owo_colors::OwoColorize;
use pgrx::prelude::*;
use pgrx_pg_config::{
    cargo::PgrxManifestExt, createdb, get_c_locale_flags, get_target_dir, PgConfig, Pgrx,
};
use postgres::error::DbError;
use std::collections::HashMap;
use std::env::VarError;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use sysinfo::{Pid, System};

mod shutdown;
pub use shutdown::add_shutdown_hook;

type LogLines = Arc<Mutex<HashMap<String, Vec<String>>>>;

struct SetupState {
    installed: bool,
    loglines: LogLines,
    system_session_id: String,
}

static TEST_MUTEX: OnceLock<Mutex<SetupState>> = OnceLock::new();

// The goal of this closure is to allow "wrapping" of anything that might issue
// an SQL simple_query or query using either a postgres::Client or
// postgres::Transaction and capture the output. The use of this wrapper is
// completely optional, but it might help narrow down some errors later on.
fn query_wrapper<F, T>(
    query: Option<String>,
    query_params: Option<&[&(dyn postgres::types::ToSql + Sync)]>,
    mut f: F,
) -> eyre::Result<T>
where
    T: IntoIterator,
    F: FnMut(
        Option<String>,
        Option<&[&(dyn postgres::types::ToSql + Sync)]>,
    ) -> Result<T, postgres::Error>,
{
    let result = f(query.clone(), query_params);

    match result {
        Ok(result) => Ok(result),
        Err(e) => {
            if let Some(dberror) = e.as_db_error() {
                let query = query.unwrap();
                let query_message = dberror.message();

                let code = dberror.code().code();
                let severity = dberror.severity();

                let mut message = format!("{severity} SQLSTATE[{code}]").bold().red().to_string();

                message.push_str(format!(": {}", query_message.bold().white()).as_str());
                message.push_str(format!("\nquery: {}", query.bold().white()).as_str());
                message.push_str(
                    format!(
                        "\nparams: {}",
                        match query_params {
                            Some(params) => format!("{params:?}"),
                            None => "None".to_string(),
                        }
                    )
                    .as_str(),
                );

                if let Ok(var) = std::env::var("RUST_BACKTRACE") {
                    if var.eq("1") {
                        let detail = dberror.detail().unwrap_or("None");
                        let hint = dberror.hint().unwrap_or("None");
                        let schema = dberror.hint().unwrap_or("None");
                        let table = dberror.table().unwrap_or("None");
                        let more_info = format!(
                            "\ndetail: {detail}\nhint: {hint}\nschema: {schema}\ntable: {table}"
                        );
                        message.push_str(more_info.as_str());
                    }
                }

                Err(eyre!(message))
            } else {
                Err(e).wrap_err("non-DbError")
            }
        }
    }
}

pub fn run_test(
    sql_funcname: &str,
    expected_error: Option<&str>,
    postgresql_conf: Vec<&'static str>,
) -> eyre::Result<()> {
    if std::env::var_os("PGRX_TEST_SKIP").unwrap_or_default() != "" {
        eprintln!(
            "Skipping test {sql_funcname:?} because `PGRX_TEST_SKIP` is set in the environment",
        );
        return Ok(());
    }
    let (loglines, system_session_id) = initialize_test_framework(postgresql_conf)?;

    let (mut client, session_id) = client()?;

    let result = client.transaction().map(|mut tx| {
        let schema = "tests"; // get_extension_schema();
        let result = tx.simple_query(&format!("SELECT \"{schema}\".\"{sql_funcname}\"();"));

        if result.is_ok() {
            // and abort the transaction when complete
            tx.rollback()?;
        }

        result
    });

    // flatten the above result
    let result = match result {
        Err(e) => Err(e),
        Ok(Err(e)) => Err(e),
        Ok(_) => Ok(()),
    };

    if let Err(e) = result {
        let error_as_string = format!("{e}");
        let cause = e.into_source();

        let (pg_location, rust_location, message) =
            if let Some(Some(dberror)) = cause.map(|e| e.downcast_ref::<DbError>().cloned()) {
                let received_error_message = dberror.message();

                if Some(received_error_message) == expected_error {
                    // the error received is the one we expected, so just return if they match
                    return Ok(());
                }

                let pg_location = dberror.file().unwrap_or("<unknown>").to_string();
                let rust_location = dberror.where_().unwrap_or("<unknown>").to_string();

                (pg_location, rust_location, received_error_message.to_string())
            } else {
                ("<unknown>".to_string(), "<unknown>".to_string(), error_as_string.to_string())
            };

        // wait a second for Postgres to get log messages written to stderr
        std::thread::sleep(std::time::Duration::from_millis(1000));

        let system_loglines = format_loglines(&system_session_id, &loglines);
        let session_loglines = format_loglines(&session_id, &loglines);
        panic!(
            "\n\nPostgres Messages:\n{system_loglines}\n\nTest Function Messages:\n{session_loglines}\n\nClient Error:\n{message}\npostgres location: {pg_location}\nrust location: {rust_location}\n\n",
                system_loglines = system_loglines.dimmed().white(),
                session_loglines = session_loglines.cyan(),
                message = message.bold().red(),
                pg_location = pg_location.dimmed().white(),
                rust_location = rust_location.yellow()
        );
    } else if let Some(message) = expected_error {
        // we expected an ERROR, but didn't get one
        return Err(eyre!("Expected error: {message}"));
    } else {
        Ok(())
    }
}

fn format_loglines(session_id: &str, loglines: &LogLines) -> String {
    let mut result = String::new();

    for line in loglines.lock().unwrap().entry(session_id.to_string()).or_default().iter() {
        result.push_str(line);
        result.push('\n');
    }

    result
}

fn initialize_test_framework(
    postgresql_conf: Vec<&'static str>,
) -> eyre::Result<(LogLines, String)> {
    let mut state = TEST_MUTEX
        .get_or_init(|| {
            Mutex::new(SetupState {
                installed: false,
                loglines: Arc::new(Mutex::new(HashMap::new())),
                system_session_id: "NONE".to_string(),
            })
        })
        .lock()
        .unwrap_or_else(|_| {
            // This used to immediately throw an std::process::exit(1), but it
            // would consume both stdout and stderr, resulting in error messages
            // not being displayed unless you were running tests with --nocapture.
            panic!(
            "Could not obtain test mutex. A previous test may have hard-aborted while holding it."
        );
        });

    if !state.installed {
        shutdown::register_shutdown_hook();
        install_extension()?;
        initdb(postgresql_conf)?;

        let system_session_id = start_pg(state.loglines.clone())?;
        let pg_config = get_pg_config()?;
        dropdb()?;
        createdb(&pg_config, get_pg_dbname(), true, false, get_runas())?;
        create_extension()?;
        state.installed = true;
        state.system_session_id = system_session_id;
    }

    Ok((state.loglines.clone(), state.system_session_id.clone()))
}

fn get_pg_config() -> eyre::Result<PgConfig> {
    let pgrx = Pgrx::from_config().wrap_err("Unable to get PGRX from config")?;

    let pg_version = pg_sys::get_pg_major_version_num();

    let pg_config = pgrx
        .get(&format!("pg{pg_version}"))
        .wrap_err_with(|| {
            format!("Error getting pg_config: {pg_version} is not a valid postgres version")
        })
        .unwrap()
        .clone();

    Ok(pg_config)
}

pub fn client() -> eyre::Result<(postgres::Client, String)> {
    let pg_config = get_pg_config()?;
    let mut client = postgres::Config::new()
        .host(pg_config.host())
        .port(pg_config.test_port().expect("unable to determine test port"))
        .user(&get_pg_user())
        .dbname(get_pg_dbname())
        .connect(postgres::NoTls)
        .wrap_err("Error connecting to Postgres")?;

    let sid_query_result = query_wrapper(
        Some("SELECT to_hex(trunc(EXTRACT(EPOCH FROM backend_start))::integer) || '.' || to_hex(pid) AS sid FROM pg_stat_activity WHERE pid = pg_backend_pid();".to_string()),
        Some(&[]),
        |query, query_params| client.query(&query.unwrap(), query_params.unwrap()),
    )
    .wrap_err("There was an issue attempting to get the session ID from Postgres")?;

    let session_id = match sid_query_result.first() {
        Some(row) => row.get::<&str, &str>("sid").to_string(),
        None => Err(eyre!("Failed to obtain a client Session ID from Postgres"))?,
    };

    query_wrapper(Some("SET log_min_messages TO 'INFO';".to_string()), None, |query, _| {
        client.simple_query(query.unwrap().as_str())
    })
    .wrap_err("Postgres Client setup failed to SET log_min_messages TO 'INFO'")?;

    query_wrapper(Some("SET log_min_duration_statement TO 1000;".to_string()), None, |query, _| {
        client.simple_query(query.unwrap().as_str())
    })
    .wrap_err("Postgres Client setup failed to SET log_min_duration_statement TO 1000;")?;

    query_wrapper(Some("SET log_statement TO 'all';".to_string()), None, |query, _| {
        client.simple_query(query.unwrap().as_str())
    })
    .wrap_err("Postgres Client setup failed to SET log_statement TO 'all';")?;

    Ok((client, session_id))
}

fn install_extension() -> eyre::Result<()> {
    let profile = std::env::var("PGRX_BUILD_PROFILE").unwrap_or("debug".into());
    let no_schema = std::env::var("PGRX_NO_SCHEMA").unwrap_or("false".into()) == "true";
    let mut features = std::env::var("PGRX_FEATURES")
        .unwrap_or("".to_string())
        .split_ascii_whitespace()
        .map(|s| s.to_string())
        .collect::<HashSet<_>>();
    features.insert("pg_test".into());

    let no_default_features =
        std::env::var("PGRX_NO_DEFAULT_FEATURES").unwrap_or("false".to_string()) == "true";
    let all_features = std::env::var("PGRX_ALL_FEATURES").unwrap_or("false".to_string()) == "true";

    let pg_version = format!("pg{}", pg_sys::get_pg_major_version_string());
    let pgrx = Pgrx::from_config()?;
    let pg_config = pgrx.get(&pg_version)?;
    let cargo_test_args = get_cargo_test_features()?;

    features.extend(cargo_test_args.features.iter().cloned());

    let mut command = cargo_pgrx();
    command
        .arg("install")
        .arg("--test")
        .arg("--pg-config")
        .arg(pg_config.path().ok_or(eyre!("No pg_config found"))?)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("CARGO_TARGET_DIR", get_target_dir()?);

    if requires_runas() {
        // if we're running tests as a different operating-system user we, then actually need to
        // install the extension artifacts as "root", as it's the user that'll definitely
        // be able to write to Postgres' various artifact directories.  "root" is also the default
        // owner of these directories for distro-managed Postgres extensions
        command.arg("--sudo");
    }

    if let Ok(manifest_path) = std::env::var("PGRX_MANIFEST_PATH") {
        command.arg("--manifest-path");
        command.arg(manifest_path);
    }

    if let Ok(rust_log) = std::env::var("RUST_LOG") {
        command.env("RUST_LOG", rust_log);
    }

    if !features.is_empty() {
        command.arg("--features");
        command.arg(features.into_iter().collect::<Vec<_>>().join(" "));
    }

    if no_default_features || cargo_test_args.no_default_features {
        command.arg("--no-default-features");
    }

    if all_features || cargo_test_args.all_features {
        command.arg("--all-features");
    }

    match profile.trim() {
        // For legacy reasons, cargo has two names for the debug profile... (We
        // also ignore the empty string here, just in case).
        "debug" | "dev" | "" => {}
        "release" => {
            command.arg("--release");
        }
        profile => {
            command.args(["--profile", profile]);
        }
    }

    if no_schema {
        command.arg("--no-schema");
    }

    let command_str = format!("{command:?}");

    let child = command.spawn().wrap_err_with(|| {
        format!("Failed to spawn process for installing extension using command: '{command_str}': ")
    })?;

    let output = child.wait_with_output().wrap_err_with(|| {
        format!(
            "Failed waiting for spawned process attempting to install extension using command: '{command_str}': "
        )
    })?;

    if !output.status.success() {
        return Err(eyre!(
            "Failure installing extension using command: {}\n\n{}{}",
            command_str,
            String::from_utf8(output.stdout).unwrap(),
            String::from_utf8(output.stderr).unwrap()
        ));
    }

    Ok(())
}

/// Maybe make the `$PGDATA` directory, if it doesn't exist.
///
/// Returns true if `initdb` should be run against the directory.
fn maybe_make_pgdata<P: AsRef<Path>>(pgdata: P) -> eyre::Result<bool> {
    let pgdata = pgdata.as_ref();
    let mut need_initdb = false;

    if let Some(runas) = get_runas() {
        // we've been asked to run as a different user.  As such, the PGDATA directory we just created
        // needs to be owned by that user.
        //
        // In order to do that, we must become that user to create it

        let mut mkdir = sudo_command(&runas);
        mkdir.arg("mkdir").arg("-p").arg(pgdata).stdout(Stdio::piped()).stderr(Stdio::piped());
        let command_str = format!("{:?}", mkdir);
        println!("{} {}", "     Running".bold().green(), command_str);
        let child = mkdir.spawn()?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            panic!(
                "failed to create the PGDATA directory at `{}`:\n{}{}",
                pgdata.display().yellow(),
                String::from_utf8(output.stdout).unwrap(),
                String::from_utf8(output.stderr).unwrap()
            );
        }

        // a PGDATA directory created as a different user will always need `initdb` to be run
        need_initdb = true;
    } else {
        // if the directory doesn't exist, make it. If it does, then we reuse it
        if !pgdata.exists() {
            std::fs::create_dir_all(&pgdata)?;

            // which is the only time we need to `initdb` it.
            need_initdb = true;
        }
    }

    Ok(need_initdb)
}

fn initdb(postgresql_conf: Vec<&'static str>) -> eyre::Result<()> {
    let pgdata = get_pgdata_path()?;

    let need_initdb = maybe_make_pgdata(&pgdata)?;

    if need_initdb {
        let pg_config = get_pg_config()?;
        let initdb_path = pg_config.initdb_path().wrap_err("unable to determine initdb path")?;

        let mut command = if let Some(runas) = get_runas() {
            let mut cmd = sudo_command(runas);
            cmd.arg(initdb_path);
            cmd
        } else {
            Command::new(initdb_path)
        };

        command
            .current_dir(pgdata.parent().unwrap())
            .args(get_c_locale_flags())
            .arg("-D")
            .arg(&pgdata)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let command_str = format!("{command:?}");

        println!("{} {}", "     Running".bold().green(), command_str);
        let child = command.spawn().wrap_err_with(|| {
            format!(
                "Failed to spawn process for initializing database using command: '{command_str}': "
            )
        })?;

        let output = child.wait_with_output().wrap_err_with(|| {
            format!(
                "Failed waiting for spawned process attempting to initialize database using command: '{command_str}': "
            )
        })?;

        if !output.status.success() {
            return Err(eyre!(
                "Failed to initialize database using command: {}\n\n{}{}",
                command_str,
                String::from_utf8(output.stdout).unwrap(),
                String::from_utf8(output.stderr).unwrap()
            ));
        }
    }

    modify_postgresql_conf(pgdata, postgresql_conf)
}

fn modify_postgresql_conf(pgdata: PathBuf, postgresql_conf: Vec<&'static str>) -> eyre::Result<()> {
    let mut contents = String::new();

    contents.push_str("log_line_prefix='[%m] [%p] [%c]: '\n");
    contents
        .push_str(&format!("unix_socket_directories = '{}'\n", pgdata.parent().unwrap().display()));
    for setting in postgresql_conf {
        contents.push_str(&format!("{setting}\n"));
    }

    let postgresql_auto_conf = pgdata.join("postgresql.auto.conf");

    if let Some(runas) = get_runas() {
        let mut sudo_command = sudo_command(&runas)
            .arg("tee")
            .arg(postgresql_auto_conf)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .wrap_err("Failed to execute sudo command")?;

        if let Some(stdin) = sudo_command.stdin.as_mut() {
            stdin.write_all(contents.as_bytes())?;
        } else {
            return Err(eyre!("Failed to get stdin for sudo command"));
        }

        let sudo_status = sudo_command.wait()?;
        if !sudo_status.success() {
            return Err(eyre!("Failed to write contents with sudo"));
        }
    } else {
        std::fs::write(postgresql_auto_conf, contents.as_bytes())?;
    }

    Ok(())
}

fn start_pg(loglines: LogLines) -> eyre::Result<String> {
    wait_for_pidfile()?;

    let pg_config = get_pg_config()?;
    let postmaster_path =
        pg_config.postmaster_path().wrap_err("unable to determine postmaster path")?;

    let mut command = if use_valgrind() {
        let mut cmd = Command::new("valgrind");
        cmd.args([
            "--leak-check=no",
            "--gen-suppressions=all",
            "--time-stamp=yes",
            "--error-markers=VALGRINDERROR-BEGIN,VALGRINDERROR-END",
            "--trace-children=yes",
        ]);
        // Try to provide a suppressions file, we'll likely get false positives
        // if we can't, but that might be better than nothing.
        if let Ok(path) = valgrind_suppressions_path(&pg_config) {
            if path.exists() {
                cmd.arg(format!("--suppressions={}", path.display()));
            }
        }

        cmd.arg(postmaster_path);
        cmd
    } else if let Some(runas) = get_runas() {
        let mut cmd = sudo_command(runas);
        cmd.arg(postmaster_path);
        cmd
    } else {
        Command::new(postmaster_path)
    };
    command
        .arg("-D")
        .arg(get_pgdata_path()?.to_str().unwrap())
        .arg("-h")
        .arg(pg_config.host())
        .arg("-p")
        .arg(pg_config.test_port().expect("unable to determine test port").to_string())
        // Redirecting logs to files can hang the test framework, override it
        .args(["-c", "log_destination=stderr", "-c", "logging_collector=off"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped());

    let command_str = format!("{command:?}");

    // start Postgres and monitor its stderr in the background
    // also notify the main thread when it's ready to accept connections
    let session_id = monitor_pg(command, command_str, loglines);

    Ok(session_id)
}

fn valgrind_suppressions_path(pg_config: &PgConfig) -> Result<PathBuf, eyre::Report> {
    let mut home = Pgrx::home()?;
    home.push(pg_config.version()?);
    home.push("src/tools/valgrind.supp");
    Ok(home)
}

fn wait_for_pidfile() -> Result<(), eyre::Report> {
    const MAX_PIDFILE_RETRIES: usize = 10;

    let pidfile = get_pid_file()?;

    let mut retries = 0;
    while pidfile.exists() {
        if retries > MAX_PIDFILE_RETRIES {
            // break out and try to start postgres anyways, maybe it'll report a decent error about what's going on
            eprintln!("`{}` has existed for ~10s.  There might be some problem with the pgrx testing Postgres instance", pidfile.display());
            break;
        }
        eprintln!("`{}` still exists.  Waiting...", pidfile.display());
        std::thread::sleep(Duration::from_secs(1));
        retries += 1;
    }
    Ok(())
}

fn monitor_pg(mut command: Command, cmd_string: String, loglines: LogLines) -> String {
    let (sender, receiver) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let mut child = command.spawn().expect("postmaster didn't spawn");

        let pid = child.id();
        // Add a shutdown hook so we can terminate it when the test framework
        // exits. TODO: Consider finding a way to handle cases where we fail to
        // clean up due to a SIGNAL?
        add_shutdown_hook(move || unsafe {
            if let Some(_runas) = get_runas() {
                sudo_command("root") // NB:  we must be "root" to kill the `sudo` process we spawned to start postgres
                    .arg("kill")
                    .arg("-s")
                    .arg("SIGTERM")
                    .arg(pid.to_string())
                    .spawn()
                    .expect("failed to spawn `sudo kill`")
                    .wait()
                    .expect("`sudo kill` didn't work");
            } else {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
                let message_string = std::ffi::CString::new(
                    format!("stopping postgres (pid={pid})\n").bold().blue().to_string(),
                )
                .unwrap();
                // IMPORTANT: Rust string literals are not naturally null-terminated
                libc::printf("%s\0".as_ptr().cast(), message_string.as_ptr());
            }
        });

        eprintln!("{cmd}\npid={p}", cmd = cmd_string.bold().blue(), p = pid.to_string().yellow());
        eprintln!("{}", pg_sys::get_pg_version_string().bold().purple());

        // wait for the database to say its ready to start up
        let reader = BufReader::new(child.stderr.take().expect("couldn't take postmaster stderr"));

        let regex = regex::Regex::new(r"\[.*?\] \[.*?\] \[(?P<session_id>.*?)\]").unwrap();
        let mut is_started_yet = false;
        let mut lines = reader.lines();
        while let Some(Ok(line)) = lines.next() {
            let session_id = get_named_capture(&regex, "session_id", &line)
                .unwrap_or_else(|| "NONE".to_string());

            if line.contains("database system is ready to accept connections") {
                // Postgres says it's ready to go
                if let Err(_) = sender.send(session_id.clone()) {
                    // The channel is closed.  This is really early in the startup process
                    // and likely indicates that a test crashed Postgres
                    panic!("{}: `monitor_pg()`:  failed to send back session_id `{session_id}`.  Did Postgres crash?", "ERROR".red().bold());
                }
                is_started_yet = true;
            }

            if !is_started_yet || line.contains("TMSG: ") {
                eprintln!("{}", line.cyan());
            }

            // if line.contains("INFO: ") {
            //     eprintln!("{}", line.cyan());
            // } else if line.contains("WARNING: ") {
            //     eprintln!("{}", line.bold().yellow());
            // } else if line.contains("ERROR: ") {
            //     eprintln!("{}", line.bold().red());
            // } else if line.contains("statement: ") || line.contains("duration: ") {
            //     eprintln!("{}", line.bold().blue());
            // } else if line.contains("LOG: ") {
            //     eprintln!("{}", line.dimmed().white());
            // } else {
            //     eprintln!("{}", line.bold().purple());
            // }

            let mut loglines = loglines.lock().unwrap();
            let session_lines = loglines.entry(session_id).or_default();
            session_lines.push(line);
        }

        // wait for Postgres to really finish
        match child.try_wait() {
            Ok(status) => {
                if let Some(_status) = status {
                    // we exited normally
                }
            }
            Err(e) => panic!("was going to let Postgres finish, but errored this time:\n{e}"),
        }
    });

    // wait for Postgres to indicate it's ready to accept connection
    // and return its pid when it is
    receiver.recv().expect("Postgres failed to start")
}

fn dropdb() -> eyre::Result<()> {
    let pg_config = get_pg_config()?;
    let dropdb_path = pg_config.dropdb_path().expect("unable to determine dropdb path");
    let mut command = if let Some(runas) = get_runas() {
        let mut cmd = sudo_command(runas);
        cmd.arg(dropdb_path);
        cmd
    } else {
        Command::new(dropdb_path)
    };

    let output = command
        .env_remove("PGDATABASE")
        .env_remove("PGHOST")
        .env_remove("PGPORT")
        .env_remove("PGUSER")
        .arg("--if-exists")
        .arg("-h")
        .arg(pg_config.host())
        .arg("-p")
        .arg(pg_config.test_port().expect("unable to determine test port").to_string())
        .arg(get_pg_dbname())
        .output()
        .unwrap();

    if !output.status.success() {
        // maybe the database didn't exist, and if so that's okay
        let stderr = String::from_utf8_lossy(output.stderr.as_slice());
        if !stderr.contains(&format!("ERROR:  database \"{}\" does not exist", get_pg_dbname())) {
            // got some error we didn't expect
            let stdout = String::from_utf8_lossy(output.stdout.as_slice());
            eprintln!("unexpected error (stdout):\n{stdout}");
            eprintln!("unexpected error (stderr):\n{stderr}");
            panic!("failed to drop test database");
        }
    }

    Ok(())
}

fn create_extension() -> eyre::Result<()> {
    let (mut client, _) = client()?;
    let extension_name = get_extension_name()?;

    query_wrapper(Some(format!("CREATE EXTENSION {extension_name} CASCADE;")), None, |query, _| {
        client.simple_query(query.unwrap().as_str())
    })
    .wrap_err(format!(
        "There was an issue creating the extension '{extension_name}' in Postgres: "
    ))?;

    Ok(())
}

fn get_extension_name() -> eyre::Result<String> {
    // We could replace this with the following if cargo adds the lib name on env var on tests/runs.
    // https://github.com/rust-lang/cargo/issues/11966
    // std::env::var("CARGO_LIB_NAME")
    //     .unwrap_or_else(|_| panic!("CARGO_LIB_NAME environment var is unset or invalid UTF-8"))
    //     .replace("-", "_")

    // CARGO_MANIFEST_DIRR — The directory containing the manifest of your package.
    // https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-crates
    let dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| eyre!("CARGO_MANIFEST_DIR environment var is unset or invalid UTF-8"))?;

    // Cargo.toml is case sensitive atm so this is ok.
    // https://github.com/rust-lang/cargo/issues/45
    let path = PathBuf::from(dir).join("Cargo.toml");
    let name = pgrx_pg_config::cargo::read_manifest(path)?.lib_name()?;
    Ok(name.replace('-', "_"))
}

fn get_pgdata_path() -> eyre::Result<PathBuf> {
    // Note that this path is turned into an entry in the unix_socket_directories config.
    // Each path has a low limit in maximum bytes, so we should avoid adding needless characters.
    let mut pgdata_base = std::env::var("CARGO_PGRX_TEST_PGDATA")
        .map(|path| PathBuf::from(path))
        .unwrap_or_else(|_| {
            let mut target_dir = get_target_dir()
                .unwrap_or_else(|e| panic!("Failed to determine the crate target directory: {e}"));
            // ./test-data or ./pgdata both seem too cryptic
            target_dir.push("test-pgdata");
            target_dir
        });

    // append the postgres version number
    pgdata_base.push(&format!("{}", pg_sys::get_pg_major_version_num()));
    Ok(pgdata_base)
}

fn get_pid_file() -> eyre::Result<PathBuf> {
    let mut pgdata = get_pgdata_path()?;
    pgdata.push("postmaster.pid");
    Ok(pgdata)
}

#[inline]
pub(crate) fn get_pg_dbname() -> &'static str {
    "pgrx_tests"
}

pub(crate) fn get_pg_user() -> String {
    get_runas().unwrap_or_else(|| {
        std::env::var("USER")
            .unwrap_or_else(|_| panic!("USER environment var is unset or invalid UTF-8"))
    })
}

#[inline]
fn get_runas() -> Option<String> {
    match std::env::var("CARGO_PGRX_TEST_RUNAS") {
        Ok(s) => Some(s),
        Err(e) => match e {
            VarError::NotPresent => None,
            VarError::NotUnicode(e) => {
                panic!(
                    "`CARGO_PGRX_TEST_RUNAS` environment var value is not unicode:  `{}`",
                    e.to_string_lossy()
                )
            }
        },
    }
}

#[inline]
fn requires_runas() -> bool {
    get_runas().is_some()
}

pub fn get_named_capture(
    regex: &regex::Regex,
    name: &'static str,
    against: &str,
) -> Option<String> {
    regex.captures(against).map(|cap| cap[name].to_string())
}

fn get_cargo_test_features() -> eyre::Result<clap_cargo::Features> {
    let mut features = clap_cargo::Features::default();
    let cargo_user_args = get_cargo_args();
    let mut iter = cargo_user_args.iter();
    while let Some(part) = iter.next() {
        match part.as_str() {
            "--no-default-features" => features.no_default_features = true,
            "--features" => {
                let configured_features = iter.next().ok_or(eyre!(
                    "no `--features` specified in the cargo argument list: {cargo_user_args:?}"
                ))?;
                features.features = configured_features
                    .split(|c: char| c.is_ascii_whitespace() || c == ',')
                    .map(|s| s.to_string())
                    .collect();
            }
            "--all-features" => features.all_features = true,
            _ => {}
        }
    }

    Ok(features)
}

fn get_cargo_args() -> Vec<String> {
    // setup the sysinfo crate's "System"
    let mut system = System::new_all();
    system.refresh_all();

    // starting with our process, look for the full set of arguments for the top-most "cargo" command
    // in our process tree.
    //
    // it's possible we've been called by:
    //  - the user from the command-line via `cargo test ...`
    //  - `cargo pgrx test ...`
    //  - `cargo test ...`
    //  - some other combination with a `cargo ...` in the middle, perhaps
    //
    // we're interested in the first arguments the **user** gave to cargo, so `framework.rs`
    // can later figure out which set of features to pass to `cargo pgrx`
    let mut pid = Pid::from(std::process::id() as usize);
    while let Some(process) = system.process(pid) {
        // only if it's "cargo"... (This works for now, but just because `cargo`
        // is at the end of the path. How *should* this handle `CARGO`?)
        if process.exe().is_some_and(|p| p.ends_with("cargo")) {
            // ... and only if it's "cargo test"...
            if process.cmd().iter().any(|arg| arg == "test")
                && !process.cmd().iter().any(|arg| arg == "pgrx")
            {
                // ... do we want its args
                return process.cmd().to_vec();
            }
        }

        // and we want to keep going to find the top-most "cargo" process in our tree
        match process.parent() {
            Some(parent_pid) => pid = parent_pid,
            None => break,
        }
    }

    Vec::new()
}

// TODO: this would be a good place to insert a check invoking to see if
// `cargo-pgrx` is a crate in the local workspace, and use it instead.
fn cargo_pgrx() -> Command {
    fn var_path(s: &str) -> Option<PathBuf> {
        std::env::var_os(s).map(PathBuf::from)
    }
    // Use `CARGO_PGRX` (set by `cargo-pgrx` on first run), then fall back to
    // `cargo-pgrx` if it is on the path, then `$CARGO pgrx`
    let cargo_pgrx = var_path("CARGO_PGRX")
        .or_else(|| find_on_path("cargo-pgrx"))
        .or_else(|| var_path("CARGO"))
        .unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo_pgrx);
    cmd.arg("pgrx");
    cmd
}

fn find_on_path(program: &str) -> Option<PathBuf> {
    assert!(!program.contains('/'));
    // Technically we should check `libc::confstr(libc::_CS_PATH)`
    // when `PATH` is unset...
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).map(|p| p.join(program)).find(|abs| abs.exists())
}

fn use_valgrind() -> bool {
    std::env::var_os("USE_VALGRIND").is_some_and(|s| s.len() > 0)
}

/// Create a [`Command`] pre-configured to what the caller decides using `sudo`
fn sudo_command<U: AsRef<OsStr>>(user: U) -> Command {
    let mut sudo = Command::new("sudo");
    sudo.arg("-u");
    sudo.arg(user);
    sudo
}
