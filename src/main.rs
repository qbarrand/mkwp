pub mod img;

use clap::Parser;
use log::info;
use serde::Deserialize;
use std::{error::Error, fs, path::Path};

fn parse_log_level(value: &str) -> Result<log::LevelFilter, String> {
    value
        .parse()
        .map_err(|_| format!("invalid log level: {value}"))
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct CommonInput {
    #[serde(rename = "fileName")]
    file_name: String,
    #[serde(rename = "isPrimary", default)]
    is_primary: bool,
    #[serde(rename = "isForLight", default)]
    is_for_light: bool,
    #[serde(rename = "isForDark", default)]
    is_for_dark: bool,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct TimeInput {
    #[serde(flatten)]
    common: CommonInput,
    time: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct SolarInput {
    #[serde(flatten)]
    common: CommonInput,
    altitude: f64,
    azimuth: f64,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Input {
    Time(TimeInput),
    Solar(SolarInput),
}

impl Input {
    pub(crate) fn file_name(&self) -> &str {
        match self {
            Self::Time(input) => &input.common.file_name,
            Self::Solar(input) => &input.common.file_name,
        }
    }

    pub(crate) fn is_primary(&self) -> bool {
        match self {
            Self::Time(input) => input.common.is_primary,
            Self::Solar(input) => input.common.is_primary,
        }
    }
}

pub fn parse_json(json: &str) -> Result<Vec<Input>, serde_json::Error> {
    serde_json::from_str::<Vec<Input>>(json)
}

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Input JSON file
    input: String,

    /// Output file
    #[arg(short, long, value_name = "FILE", default_value = "output.heif")]
    output: String,

    /// HEVC encoding speed/compression tradeoff
    #[arg(long = "libheif-preset", value_enum, default_value_t = img::Preset::Slow)]
    preset: img::Preset,

    /// Minimum log level
    #[arg(
        short,
        long,
        value_name = "LEVEL",
        value_parser = parse_log_level,
        default_value = "info"
    )]
    log_level: log::LevelFilter,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    env_logger::builder().filter_level(args.log_level).init();

    info!(path = &args.input.as_str(); "Reading input file");

    let json = fs::read_to_string(&args.input)?;
    let input = parse_json(&json)?;

    info!(count = input.len(), preset = format!("{:?}", args.preset); "Encoding HEIF images");
    img::build_heif(
        &input,
        Path::new(&args.input),
        Path::new(&args.output),
        args.preset,
    )?;
    info!(path = &args.output.as_str(); "Wrote HEIF file");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Args, CommonInput, Input, SolarInput, TimeInput, img::Preset, parse_json};
    use clap::Parser;

    #[test]
    fn parses_heif_preset() {
        let args =
            Args::try_parse_from(["mkwp", "input.json", "--libheif-preset", "fast"]).unwrap();

        assert_eq!(args.preset, Preset::Fast);
    }

    #[test]
    fn parses_time_json() {
        let times = parse_json(include_str!("../sample/time.json")).unwrap();

        assert_eq!(
            times,
            vec![
                Input::Time(TimeInput {
                    common: CommonInput {
                        file_name: "1.png".into(),
                        is_primary: true,
                        is_for_light: true,
                        is_for_dark: false,
                    },
                    time: "10:25:43".into(),
                }),
                Input::Time(TimeInput {
                    common: CommonInput {
                        file_name: "2.png".into(),
                        is_primary: false,
                        is_for_light: false,
                        is_for_dark: false,
                    },
                    time: "14:32:12".into(),
                }),
                Input::Time(TimeInput {
                    common: CommonInput {
                        file_name: "3.png".into(),
                        is_primary: false,
                        is_for_light: false,
                        is_for_dark: false,
                    },
                    time: "18:12:01".into(),
                }),
                Input::Time(TimeInput {
                    common: CommonInput {
                        file_name: "4.png".into(),
                        is_primary: false,
                        is_for_light: false,
                        is_for_dark: true,
                    },
                    time: "20:10:45".into(),
                }),
            ]
        );
    }

    #[test]
    fn parses_solar_json() {
        let solar = parse_json(include_str!("../sample/solar.json")).unwrap();

        assert_eq!(
            solar,
            vec![
                Input::Solar(SolarInput {
                    common: CommonInput {
                        file_name: "1.png".into(),
                        is_primary: true,
                        is_for_light: true,
                        is_for_dark: false,
                    },
                    altitude: 27.95,
                    azimuth: 279.66,
                }),
                Input::Solar(SolarInput {
                    common: CommonInput {
                        file_name: "2.png".into(),
                        is_primary: false,
                        is_for_light: false,
                        is_for_dark: false,
                    },
                    altitude: -31.05,
                    azimuth: 4.16,
                }),
                Input::Solar(SolarInput {
                    common: CommonInput {
                        file_name: "16.png".into(),
                        is_primary: false,
                        is_for_light: false,
                        is_for_dark: true,
                    },
                    altitude: -28.63,
                    azimuth: 340.41,
                }),
            ]
        );
    }
}
