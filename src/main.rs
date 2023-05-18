use core::time::Duration;
use dotenv::dotenv;
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::io::{Error, ErrorKind, Read};

#[derive(PartialEq, Debug)]
enum Status {
    Online,
    Offline,
    Errored,
    Unknown,
}

#[derive(Debug)]
struct App {
    pub name: String,
    pub last_status: Status,
    pub led_num: Option<i8>,
}

impl App {
    pub fn new(name: String, last_status: Status, led_num: Option<i8>) -> App {
        App {
            name,
            last_status,
            led_num,
        }
    }
}

#[derive(Deserialize, Debug)]
struct ParticleFnResult {
    id: String,
    name: String,
    connected: bool,
    return_value: isize,
}

fn main() {
    dotenv().ok();

    let token = env::var("ACCESS_TOKEN").expect("Please provide a Particle access token!");

    let mut app_statuses: HashMap<String, App> = HashMap::new();
    let mut available_led_nums: Vec<i8> = (1..12).collect();

    loop {
        std::thread::sleep(Duration::from_secs(4));

        let status_map = get_statuses();

        // Frees up an LED if a process status is no longer present.
        let app_names: Vec<String> = app_statuses.keys().cloned().collect();
        for app_name in app_names {
            if !status_map.iter().any(|(name, _)| name == &app_name) {
                if let Some((
                    _,
                    App {
                        led_num: Some(led), ..
                    },
                )) = app_statuses.remove_entry(&app_name)
                {
                    available_led_nums.push(led);
                }
            }
        }

        // Iterate through the list of processes and turn on LEDs to reflect their state.
        for (app_name, status) in status_map {
            let app = app_statuses.entry(app_name.clone()).or_insert(App::new(
                app_name,
                Status::Unknown,
                available_led_nums.pop(),
            ));

            if app.last_status != status {
                update_app(&token, app, status)
            }
        }
    }
}

fn update_app(token: &String, app: &mut App, new_status: Status) {
    app.last_status = new_status;
    if let (Some(led), Ok(device)) = (app.led_num, env::var("DEVICE_NAME")) {
        let to_call = get_status_fn(&app.last_status);
        let url = format!("https://api.particle.io/v1/devices/{}/{}", device, to_call);

        // Apparently I have to create the client every time, because you can't change the URL after creation.....
        let client = reqwest::blocking::Client::new()
            .post(url)
            .bearer_auth(token)
            .json(&HashMap::from([("arg", format!("{}", led))]));

        if let Err(error) = client
            .send()
            .and_then(|resp| resp.json::<ParticleFnResult>())
            .and_then(|result| {
                println!("Successfully updated LED {}", result.return_value);
                Ok(())
            })
        {
            println!("Error when calling Particle Cloud fn: {:?}", error);
        }
    }
}

fn get_status_fn(status: &Status) -> String {
    match status {
        Status::Online => "setOnline",
        Status::Offline => "setOffline",
        Status::Errored => "setOffline",
        Status::Unknown => "setUndefined",
    }
    .to_string()
}

fn get_statuses() -> Vec<(String, Status)> {
    env::var("APPS")
        .unwrap_or(String::new())
        .split(",")
        .filter_map(|service_name| {
            if let Ok(capture) = systemctl_capture(vec!["status", service_name]) {
                Some((
                    service_name.to_string(),
                    systemctl_capture_to_status(capture),
                ))
            } else {
                None
            }
        })
        .collect()
}

fn systemctl_capture_to_status(capture: String) -> Status {
    // need to change this to be more accurate. Need to find out if "inactive" changes when it's offline out of an error.
    if capture.contains("Active: active") {
        Status::Online
    } else if capture.contains("Active: inactive") {
        Status::Offline
    } else {
        Status::Errored
    }
}

// from https://docs.rs/systemctl/latest/src/systemctl/lib.rs.html#22-58
/// Invokes `systemctl $args` and captures stdout stream
fn systemctl_capture(args: Vec<&str>) -> std::io::Result<String> {
    let mut child = std::process::Command::new("/usr/bin/systemctl")
        .args(args.clone())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let _exitcode = child.wait()?;
    //TODO improve this please
    //Interrogating some services returns an error code
    //match exitcode.success() {
    //true => {
    let mut stdout: Vec<u8> = Vec::new();
    if let Ok(size) = child.stdout.unwrap().read_to_end(&mut stdout) {
        if size > 0 {
            if let Ok(s) = String::from_utf8(stdout) {
                Ok(s)
            } else {
                Err(Error::new(
                    ErrorKind::InvalidData,
                    "Invalid utf8 data in stdout",
                ))
            }
        } else {
            Err(Error::new(ErrorKind::InvalidData, "systemctl stdout empty"))
        }
    } else {
        Err(Error::new(ErrorKind::InvalidData, "systemctl stdout empty"))
    }
    /*},
        false => {
            Err(Error::new(ErrorKind::Other,
                format!("/usr/bin/systemctl {:?} failed", args)))
        }
    }*/
}
