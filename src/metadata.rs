//! Metadata payloads used by macOS Dynamic Desktop HEIF files.

use plist::{Dictionary, Value};
use thiserror::Error;

use crate::Input;

/// Builds the binary-plist-ready payload for the `apple_desktop:h24` XMP tag.
///
/// The caller is responsible for serializing this value as a binary plist,
/// Base64-encoding it, and embedding it into the HEIF's XMP metadata.
pub fn time_plist(inputs: &[Input]) -> Result<Value, TimeMetadataError> {
    let mut time_information = Vec::with_capacity(inputs.len());
    let mut light_index = None;
    let mut dark_index = None;

    for (index, input) in inputs.iter().enumerate() {
        let time = input.time().ok_or(TimeMetadataError::ContainsSolarInput)?;
        let index = u64::try_from(index).expect("a slice cannot exceed u64::MAX entries");

        let mut entry = Dictionary::new();
        entry.insert("i".into(), Value::Integer(index.into()));
        entry.insert("t".into(), Value::Real(time_fraction(time)?));
        time_information.push(Value::Dictionary(entry));

        if input.is_for_light() && light_index.replace(index).is_some() {
            return Err(TimeMetadataError::MultipleLightImages);
        }
        if input.is_for_dark() && dark_index.replace(index).is_some() {
            return Err(TimeMetadataError::MultipleDarkImages);
        }
    }

    let mut payload = Dictionary::new();
    payload.insert("ti".into(), Value::Array(time_information));

    match (light_index, dark_index) {
        (None, None) => {}
        (Some(light), Some(dark)) => {
            let mut appearance = Dictionary::new();
            appearance.insert("l".into(), Value::Integer(light.into()));
            appearance.insert("d".into(), Value::Integer(dark.into()));
            payload.insert("ap".into(), Value::Dictionary(appearance));
        }
        _ => return Err(TimeMetadataError::IncompleteAppearanceMapping),
    }

    Ok(Value::Dictionary(payload))
}

fn time_fraction(time: &str) -> Result<f64, TimeMetadataError> {
    let mut parts = time.split(':');
    let hour = parse_time_part(parts.next(), time)?;
    let minute = parse_time_part(parts.next(), time)?;
    let second = parse_time_part(parts.next(), time)?;
    if parts.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return Err(TimeMetadataError::InvalidTime { value: time.into() });
    }

    Ok((hour * 3_600 + minute * 60 + second) as f64 / 86_400.0)
}

fn parse_time_part(part: Option<&str>, whole_time: &str) -> Result<u32, TimeMetadataError> {
    part.and_then(|part| part.parse().ok())
        .ok_or_else(|| TimeMetadataError::InvalidTime {
            value: whole_time.into(),
        })
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TimeMetadataError {
    #[error("time metadata cannot contain a solar input")]
    ContainsSolarInput,
    #[error("invalid time '{value}': expected HH:MM:SS")]
    InvalidTime { value: String },
    #[error("time metadata identifies more than one light image")]
    MultipleLightImages,
    #[error("time metadata identifies more than one dark image")]
    MultipleDarkImages,
    #[error("time metadata must identify both a light and dark image, or neither")]
    IncompleteAppearanceMapping,
}

#[cfg(test)]
mod tests {
    use plist::{Dictionary, Value};

    use super::{TimeMetadataError, time_plist};

    #[test]
    fn builds_a_time_metadata_payload() {
        let inputs = crate::parse_json(include_str!("../sample/time.json")).unwrap();

        let metadata = time_plist(&inputs).unwrap();
        let payload = metadata.as_dictionary().unwrap();
        let time_information = payload["ti"].as_array().unwrap();

        assert_eq!(time_information.len(), 4);
        assert_eq!(
            time_information[0].as_dictionary().unwrap()["i"],
            Value::Integer(0.into())
        );
        assert_eq!(
            time_information[0].as_dictionary().unwrap()["t"],
            Value::Real(37_543.0 / 86_400.0)
        );

        let mut expected_appearance = Dictionary::new();
        expected_appearance.insert("l".into(), Value::Integer(0.into()));
        expected_appearance.insert("d".into(), Value::Integer(3.into()));
        assert_eq!(payload["ap"], Value::Dictionary(expected_appearance));
    }

    #[test]
    fn rejects_invalid_time() {
        let inputs = crate::parse_json(r#"[{"fileName":"day.png","time":"25:00:00"}]"#).unwrap();

        assert_eq!(
            time_plist(&inputs),
            Err(TimeMetadataError::InvalidTime {
                value: "25:00:00".into()
            })
        );
    }

    #[test]
    fn rejects_an_incomplete_appearance_mapping() {
        let inputs =
            crate::parse_json(r#"[{"fileName":"day.png","isForLight":true,"time":"08:00:00"}]"#)
                .unwrap();

        assert_eq!(
            time_plist(&inputs),
            Err(TimeMetadataError::IncompleteAppearanceMapping)
        );
    }
}
