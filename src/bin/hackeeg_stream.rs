use log::info;
use std::time::Duration;

use clap::{App, AppSettings, Arg};
use serialport::prelude::SerialPortSettings;

use common::constants::ads1299;
use hackeeg::client::commands::responses::Status;
use hackeeg::{client::modes::Mode, client::HackEEGClient, common};

const MAIN_TAG: &str = "main";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = App::new("HackEEG Streamer")
        .about("Reads data from a serial port and echoes it to stdout")
        .setting(AppSettings::DisableVersion)
        .arg(
            Arg::with_name("verbosity")
                .short("v")
                .multiple(true)
                .help("Sets the level of verbosity"),
        )
        .arg(
            Arg::with_name("port")
                .help("The device path to a serial port")
                .use_delimiter(false)
                .required(true),
        )
        .arg(
            Arg::with_name("baud")
                .help("The baud rate to connect at")
                .use_delimiter(false)
                .default_value("115200")
                .required(true),
        )
        .get_matches();

    let log_level = match matches.occurrences_of("verbosity") {
        0 => log::LevelFilter::Info,
        1 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    };

    let port_name = matches.value_of("port").unwrap();
    let baud_rate = matches.value_of("baud").unwrap().parse::<u32>()?;

    common::log::setup_logger(log_level, None)?;

    let mut settings = SerialPortSettings::default();
    settings.baud_rate = baud_rate;
    settings.timeout = Duration::from_millis(10);

    let mut client = HackEEGClient::new(port_name, &settings)?;

    client.sdatac();

    let sample_mode = ads1299::Speed::HIGH_RES_500_SPS as u8 | ads1299::CONFIG1_const;
    client
        .wreg::<Status>(ads1299::GlobalSettings::CONFIG1 as u8, sample_mode)?
        .assert()?;

    info!(target: MAIN_TAG, "Disabling all channels");
    client.disable_all_channels()?;

    info!(target: MAIN_TAG, "Enabling channel config test");
    client.channel_config_test()?;

    // Route reference electrode to SRB1: JP8:1-2, JP7:NC (not connected)
    // use this with humans to reduce noise
    info!(target: MAIN_TAG, "Enabling reference electrode SRB1");
    client
        .wreg::<Status>(ads1299::MISC1, ads1299::SRB1 | ads1299::MISC1_const)?
        .assert()?;

    // Single-ended mode - setting SRB1 bit sends mid-supply voltage to the N inputs
    // use this with a signal generator
    // client.wreg(ads1299::MISC1, ads1299::SRB1)?;

    // Dual-ended mode
    info!(target: MAIN_TAG, "Setting dual-ended mode");
    client
        .wreg::<Status>(ads1299::MISC1, ads1299::MISC1_const)?
        .assert()?;

    // add channels into bias generation
    // self.hackeeg.wreg(ads1299.BIAS_SENSP, ads1299.BIAS8P)
    //
    //    if messagepack:
    //        self.hackeeg.messagepack_mode()
    //    else:
    //    self.hackeeg.jsonlines_mode()
    //    self.hackeeg.start()
    //    self.hackeeg.rdatac()

    client.start()?;
    client.rdatac()?;

    let resp = client.read_rdatac_response()?;

    Ok(())
}
