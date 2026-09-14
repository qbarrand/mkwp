//! Metadata payloads used by macOS Dynamic Desktop HEIF files.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use plist::{Dictionary, Value};
use thiserror::Error;

use crate::Input;

const XMP_PREFIX: &str = r#"<?xpacket begin="﻿" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:apple_desktop="http://ns.apple.com/namespace/1.0/" apple_desktop:h24=""#;
const XMP_SUFFIX: &str = r#""/>
</rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;

/// Builds an XMP packet containing the Dynamic Desktop metadata for `inputs`.
pub fn xmp(inputs: &[Input]) -> Result<Vec<u8>, MetadataError> {
    let payload = match inputs.first() {
        Some(Input::Time(_)) => time_plist(inputs)?,
        Some(Input::Solar(_)) => solar_plist(inputs)?,
        None => return Err(MetadataError::NoImages),
    };

    let mut binary_plist = Vec::new();
    plist::to_writer_binary(&mut binary_plist, &payload)?;

    let encoded = STANDARD.encode(binary_plist);
    let mut packet = Vec::with_capacity(XMP_PREFIX.len() + encoded.len() + XMP_SUFFIX.len());
    packet.extend_from_slice(XMP_PREFIX.as_bytes());
    packet.extend_from_slice(encoded.as_bytes());
    packet.extend_from_slice(XMP_SUFFIX.as_bytes());
    Ok(packet)
}

/// Builds the binary-plist-ready payload for the `apple_desktop:h24` XMP tag.
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

        record_appearance(
            input,
            index,
            &mut light_index,
            &mut dark_index,
            TimeMetadataError::MultipleLightImages,
            TimeMetadataError::MultipleDarkImages,
        )?;
    }

    let mut payload = Dictionary::new();
    payload.insert("ti".into(), Value::Array(time_information));
    insert_appearance(
        &mut payload,
        light_index,
        dark_index,
        TimeMetadataError::IncompleteAppearanceMapping,
    )?;
    Ok(Value::Dictionary(payload))
}

/// Builds the binary-plist-ready solar payload for the `apple_desktop:h24` XMP tag.
pub fn solar_plist(inputs: &[Input]) -> Result<Value, SolarMetadataError> {
    let mut solar_information = Vec::with_capacity(inputs.len());
    let mut light_index = None;
    let mut dark_index = None;

    for (index, input) in inputs.iter().enumerate() {
        let (altitude, azimuth) = input
            .solar_position()
            .ok_or(SolarMetadataError::ContainsTimeInput)?;
        let index = u64::try_from(index).expect("a slice cannot exceed u64::MAX entries");

        let mut entry = Dictionary::new();
        entry.insert("i".into(), Value::Integer(index.into()));
        entry.insert("a".into(), Value::Real(altitude));
        entry.insert("z".into(), Value::Real(azimuth));
        solar_information.push(Value::Dictionary(entry));

        record_appearance(
            input,
            index,
            &mut light_index,
            &mut dark_index,
            SolarMetadataError::MultipleLightImages,
            SolarMetadataError::MultipleDarkImages,
        )?;
    }

    let mut payload = Dictionary::new();
    payload.insert("si".into(), Value::Array(solar_information));
    insert_appearance(
        &mut payload,
        light_index,
        dark_index,
        SolarMetadataError::IncompleteAppearanceMapping,
    )?;
    Ok(Value::Dictionary(payload))
}

fn record_appearance<E>(
    input: &Input,
    index: u64,
    light_index: &mut Option<u64>,
    dark_index: &mut Option<u64>,
    multiple_light: E,
    multiple_dark: E,
) -> Result<(), E> {
    if input.is_for_light() && light_index.replace(index).is_some() {
        return Err(multiple_light);
    }
    if input.is_for_dark() && dark_index.replace(index).is_some() {
        return Err(multiple_dark);
    }
    Ok(())
}

fn insert_appearance<E>(
    payload: &mut Dictionary,
    light_index: Option<u64>,
    dark_index: Option<u64>,
    incomplete: E,
) -> Result<(), E> {
    match (light_index, dark_index) {
        (None, None) => Ok(()),
        (Some(light), Some(dark)) => {
            let mut appearance = Dictionary::new();
            appearance.insert("l".into(), Value::Integer(light.into()));
            appearance.insert("d".into(), Value::Integer(dark.into()));
            payload.insert("ap".into(), Value::Dictionary(appearance));
            Ok(())
        }
        _ => Err(incomplete),
    }
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

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("metadata cannot be generated without images")]
    NoImages,
    #[error(transparent)]
    Time(#[from] TimeMetadataError),
    #[error(transparent)]
    Solar(#[from] SolarMetadataError),
    #[error("could not serialize metadata: {0}")]
    Serialize(#[from] plist::Error),
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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SolarMetadataError {
    #[error("solar metadata cannot contain a time input")]
    ContainsTimeInput,
    #[error("solar metadata identifies more than one light image")]
    MultipleLightImages,
    #[error("solar metadata identifies more than one dark image")]
    MultipleDarkImages,
    #[error("solar metadata must identify both a light and dark image, or neither")]
    IncompleteAppearanceMapping,
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use plist::{Dictionary, Value};

    use super::{TimeMetadataError, solar_plist, time_plist, xmp};

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
    fn builds_a_solar_metadata_payload() {
        let inputs = crate::parse_json(include_str!("../sample/solar.json")).unwrap();
        let metadata = solar_plist(&inputs).unwrap();
        let solar = metadata.as_dictionary().unwrap()["si"].as_array().unwrap();

        assert_eq!(solar.len(), 3);
        assert_eq!(solar[0].as_dictionary().unwrap()["a"], Value::Real(27.95));
        assert_eq!(solar[0].as_dictionary().unwrap()["z"], Value::Real(279.66));
    }

    #[test]
    fn wraps_the_binary_plist_in_xmp() {
        let inputs = crate::parse_json(include_str!("../sample/time.json")).unwrap();
        let packet = String::from_utf8(xmp(&inputs).unwrap()).unwrap();
        let encoded = packet
            .split_once("apple_desktop:h24=\"")
            .unwrap()
            .1
            .split_once('"')
            .unwrap()
            .0;
        let plist =
            Value::from_reader(std::io::Cursor::new(STANDARD.decode(encoded).unwrap())).unwrap();

        assert_eq!(plist, time_plist(&inputs).unwrap());
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
