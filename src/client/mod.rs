use log::{debug, info, trace};
use serde_json::json;
use serialport::prelude::*;
use serialport::Result as SerialResult;
use std::cell::{Cell, RefCell};
use std::error::Error;
use std::io::Result as IOResult;
use std::io::{BufRead, BufReader};
use std::time::Duration;

mod commands;
mod err;
mod modes;

use crate::client::err::ClientError;
use modes::Mode;

const CLIENT_TAG: &str = "hackeeg_client";

struct Port {
    raw_port: Box<dyn SerialPort>,
    reader: BufReader<Box<dyn SerialPort>>,
}

pub struct HackEEGClient {
    port_name: String,
    port: RefCell<Box<dyn SerialPort>>,
    mode: Mode,
}

type ClientResult<T> = Result<T, err::ClientError>;

impl HackEEGClient {
    pub fn new(port_name: &str, settings: &SerialPortSettings) -> Result<Self, Box<dyn Error>> {
        let port = serialport::open_with_settings(port_name, settings)?;

        // construct our client
        let mut client = Self {
            port_name: port_name.to_string(),
            port: RefCell::new(port),
            mode: Mode::Unknown,
        };
        client.ensure_mode(Mode::JsonLines);

        Ok(client)
    }

    pub fn jsonlines(&self) -> IOResult<usize> {
        self.send_text_cmd("jsonlines")
    }

    pub fn blink_test(&self, num: u32) -> IOResult<()> {
        info!("Starting blink test.");
        let sleep = || std::thread::sleep(std::time::Duration::from_millis(100));
        for i in 0..num {
            info!("Blinking {} more times", num - i);
            self.board_led_on()?;
            sleep();
            self.board_led_off()?;
            sleep();
        }
        Ok(())
    }

    pub fn noop(&self) -> ClientResult<bool> {
        // no-op is a little special in that it can be expected to fail on deserialization, and
        // that isn't considered an error
        match self.execute_json_cmd("nop") {
            Ok(commands::NoOp {
                status_code,
                status_text,
            }) => Ok(true),
            Err(ClientError::DeserializeError(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub fn board_led_on(&self) -> IOResult<()> {
        info!(target: CLIENT_TAG, "Turning board LED on");
        self.send_json_cmd("boardledon")?;
        Ok(())
    }

    pub fn board_led_off(&self) -> IOResult<()> {
        info!(target: CLIENT_TAG, "Turning board LED off");
        self.send_json_cmd("boardledoff")?;
        Ok(())
    }

    pub fn send_json_cmd(&self, cmd: &str) -> IOResult<usize> {
        debug!(target: CLIENT_TAG, "Sending JSON command '{}'", cmd);
        self.port.borrow_mut().write(json_cmd_line(cmd).as_bytes())
    }

    pub fn send_text_cmd(&self, cmd: &str) -> IOResult<usize> {
        debug!(target: CLIENT_TAG, "Sending text command '{}'", cmd);
        let mut port = self.port.borrow_mut();
        let mut full_cmd = cmd.to_string();
        full_cmd.push('\n');
        port.write(full_cmd.as_bytes())
    }

    fn read_response(&self) -> IOResult<String> {
        let mut port = self.port.borrow_mut();
        let mut reader = BufReader::new(port.as_mut());
        let mut buf = String::new();
        reader.read_line(&mut buf)?;

        Ok(buf)
    }

    /// Executes a json command and deserializes the result as `T`.  Since `T` has
    /// `DeserializeOwned`, this performs a copy.  For very high performance, write another function
    /// that passes in the buffer and bounds `T` with `Deserialize<'de>` instead, for no copies.
    pub fn execute_json_cmd<T>(&self, cmd: &str) -> ClientResult<T>
    where
        T: serde::de::DeserializeOwned + Clone,
    {
        debug!(
            target: CLIENT_TAG,
            "Executing JSON command '{}' and then reading response", cmd
        );
        self.send_json_cmd(cmd)?;

        let mut buf = vec![0; 1024];
        let resp = self.read_response()?;
        trace!(target: CLIENT_TAG, "Got response: {}", resp.trim());

        Ok(serde_json::from_str(&resp)?)
    }

    /// Ensures that the device is in the desired mode, and returns whether it had to change it
    /// into that mode in order to ensure
    fn ensure_mode(&mut self, desired_mode: Mode) -> ClientResult<bool> {
        info!(target: CLIENT_TAG, "Ensuring we're in mode {:?}", self.mode);
        if self.mode != desired_mode {
            debug!(
                target: CLIENT_TAG,
                "Desired mode {:?} doesn't match current mode {:?}", desired_mode, self.mode
            );

            match desired_mode {
                Mode::Text => match self.mode {
                    Mode::JsonLines => {
                        self.send_text_cmd("jsonlines")?;
                    }
                    Mode::MsgPack => {
                        self.send_text_cmd("jsonlines")?;
                        self.send_text_cmd("messagepack")?;
                    }
                    _ => unreachable!(),
                },
                Mode::JsonLines => match self.mode {
                    Mode::MsgPack => {
                        self.send_text_cmd("messagepack")?;
                    }
                    Mode::Text | Mode::Unknown => {
                        self.send_json_cmd("stop")?;
                        self.send_json_cmd("sdatac")?;
                        self.send_text_cmd("jsonlines")?;
                        self.noop()?;
                    }
                    _ => unreachable!(),
                },
                Mode::MsgPack => match self.mode {
                    Mode::JsonLines => {
                        self.send_text_cmd("jsonlines")?;
                    }
                    Mode::Text => {
                        self.send_json_cmd("text")?;
                    }
                    _ => unreachable!(),
                },
                // we should never get here, because our new() method determines the current mode
                Mode::Unknown => unreachable!(),
            }

            self.mode = desired_mode;
            Ok(true)
        } else {
            debug!(target: CLIENT_TAG, "We're already in mode {:?}", self.mode);
            Ok(false)
        }
    }
}

fn json_cmd(cmd: &str) -> String {
    let value = json!({
        "COMMAND": cmd,
        "PARAMETERS": [],
    });
    return value.to_string();
}

fn json_cmd_line(cmd: &str) -> String {
    json_cmd(cmd) + "\r\n"
}