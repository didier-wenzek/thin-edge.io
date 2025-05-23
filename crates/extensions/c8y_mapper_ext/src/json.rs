//! A library to translate the ThinEdgeJson into C8yJson
//! Takes thin_edge_json bytes and returns c8y json bytes
//!
//! # Examples
//!
//! ```
//! use c8y_mapper_ext::json::from_thin_edge_json;
//! use c8y_mapper_ext::entity_cache::CloudEntityMetadata;
//! use tedge_api::entity::EntityMetadata;
//! let single_value_thin_edge_json = r#"{
//!        "time": "2020-06-22T17:03:14.000+02:00",
//!        "temperature": 23,
//!        "pressure": 220
//!     }"#;
//! let entity = CloudEntityMetadata::new("foo".into(), EntityMetadata::main_device(None));
//! let output = from_thin_edge_json(single_value_thin_edge_json, &entity,"");
//! ```

use crate::entity_cache::CloudEntityMetadata;
use crate::serializer;
use clock::Clock;
use clock::WallClock;
use tedge_api::measurement::*;
use time::OffsetDateTime;
use time::{self};

#[derive(thiserror::Error, Debug)]
pub enum CumulocityJsonError {
    #[error(transparent)]
    C8yJsonSerializationError(#[from] serializer::C8yJsonSerializationError),

    #[error(transparent)]
    ThinEdgeJsonParserError(#[from] ThinEdgeJsonParserError),
}

/// Converts from thin-edge measurement JSON to C8Y measurement JSON
pub fn from_thin_edge_json(
    input: &str,
    entity: &CloudEntityMetadata,
    m_type: &str,
) -> Result<String, CumulocityJsonError> {
    let timestamp = WallClock.now();
    let c8y_vec = from_thin_edge_json_with_timestamp(input, timestamp, entity, m_type)?;
    Ok(c8y_vec)
}

fn from_thin_edge_json_with_timestamp(
    input: &str,
    timestamp: OffsetDateTime,
    entity: &CloudEntityMetadata,
    m_type: &str,
) -> Result<String, CumulocityJsonError> {
    let mut serializer = serializer::C8yJsonSerializer::new(timestamp, entity, m_type);
    parse_str(input, &mut serializer)?;
    Ok(serializer.into_string()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_json_diff::*;
    use proptest::prelude::*;
    use serde_json::json;
    use serde_json::Value;
    use tedge_api::entity::EntityMetadata;
    use test_case::test_case;
    use time::format_description;
    use time::macros::datetime;

    #[test]
    fn check_single_value_translation() {
        let single_value_thin_edge_json = r#"{
                  "temperature": 23.0,
                  "pressure": 220.0
               }"#;

        let timestamp = datetime!(2021-04-08 0:00:0 +05:00);

        let entity = CloudEntityMetadata::new("foo".into(), EntityMetadata::main_device(None));
        let output =
            from_thin_edge_json_with_timestamp(single_value_thin_edge_json, timestamp, &entity, "");

        let expected_output = json!({
            "time": timestamp
                .format(&format_description::well_known::Rfc3339)
                .unwrap()
                .as_str(),
            "temperature": {
                "temperature": {
                    "value": 23.0
                }
            },
            "pressure": {
                "pressure": {
                    "value": 220.0
                }
            },
            "type": "ThinEdgeMeasurement"
        });

        assert_json_eq!(
            serde_json::from_str::<serde_json::Value>(output.unwrap().as_str()).unwrap(),
            expected_output
        );
    }

    #[test]
    fn check_type_translation() {
        let single_value_thin_edge_json = r#"{
                  "type": "test",
                  "temperature": 23.0               
               }"#;

        let timestamp = datetime!(2021-04-08 0:00:0 +05:00);

        let entity = CloudEntityMetadata::new("foo".into(), EntityMetadata::main_device(None));
        let output =
            from_thin_edge_json_with_timestamp(single_value_thin_edge_json, timestamp, &entity, "");

        let expected_output = json!({
            "time": timestamp
                .format(&format_description::well_known::Rfc3339)
                .unwrap()
                .as_str(),
            "temperature": {
                "temperature": {
                    "value": 23.0
                }
            },
            "type": "test"
        });

        assert_json_eq!(
            serde_json::from_str::<serde_json::Value>(output.unwrap().as_str()).unwrap(),
            expected_output
        );
    }

    #[test]
    fn check_thin_edge_translation_with_timestamp() {
        let single_value_thin_edge_json = r#"{
                  "time" : "2013-06-22T17:03:14.123+02:00",
                  "temperature": 23.0,
                  "pressure": 220.0
               }"#;

        let expected_output = r#"{
                     "time": "2013-06-22T17:03:14.123+02:00",
                     "temperature": {
                         "temperature": {
                               "value": 23.0
                         }
                    },
                    "pressure" : {
                       "pressure": {
                          "value" : 220.0
                          }
                       },
                    "type": "ThinEdgeMeasurement"
                  }"#;

        let entity = CloudEntityMetadata::new("foo".into(), EntityMetadata::main_device(None));
        let output = from_thin_edge_json(single_value_thin_edge_json, &entity, "");

        assert_eq!(
            expected_output.split_whitespace().collect::<String>(),
            output.unwrap().split_whitespace().collect::<String>()
        );
    }

    #[test]
    fn check_multi_value_translation() {
        let multi_value_thin_edge_json = r#"{
            "temperature": 25.0 ,
            "location": {
                  "latitude": 32.54,
                  "longitude": -117.67,
                  "altitude": 98.6
              },
            "pressure": 98.0
        }"#;

        let timestamp = datetime!(2021-04-08 0:00:0 +05:00);

        let entity = CloudEntityMetadata::new("foo".into(), EntityMetadata::main_device(None));
        let output =
            from_thin_edge_json_with_timestamp(multi_value_thin_edge_json, timestamp, &entity, "");

        let expected_output = json!({
            "time": timestamp
                .format(&format_description::well_known::Rfc3339)
                .unwrap()
                .as_str(),
            "temperature": {
                "temperature": {
                    "value": 25.0
                 }
            },
           "location": {
                "latitude": {
                   "value": 32.54
                 },
                "longitude": {
                  "value": -117.67
                },
                "altitude": {
                  "value": 98.6
               }
          },
         "pressure": {
            "pressure": {
                 "value": 98.0
            }
          },
          "type": "ThinEdgeMeasurement"
        });

        assert_json_eq!(
            serde_json::from_str::<serde_json::Value>(output.unwrap().as_str()).unwrap(),
            expected_output
        );
    }

    #[test]
    fn thin_edge_json_round_tiny_number() {
        let input = r#"{
           "time" : "2013-06-22T17:03:14.000+02:00",
           "temperature": 10e-9999999999
          }"#;

        let expected_output = r#"{
             "time": "2013-06-22T17:03:14+02:00",
             "temperature": {
                 "temperature": {
                    "value": 0.0
                 }
            },
            "type": "ThinEdgeMeasurement"
        }"#;

        let entity = CloudEntityMetadata::new("foo".into(), EntityMetadata::main_device(None));
        let output = from_thin_edge_json(input, &entity, "");

        let actual_output = output.unwrap().split_whitespace().collect::<String>();

        assert_eq!(
            expected_output.split_whitespace().collect::<String>(),
            actual_output
        );
    }

    proptest! {

        #[test]
        fn it_works_for_any_measurement(measurement in r#"[a-z]{3,6}"#) {
            if measurement == "time" || measurement == "type" {
                // Skip this test case, since the random measurement name happens to be a reserved key.
                return Ok(());
            }
            let input = format!(r#"{{"time": "2013-06-22T17:03:14.453+02:00",
                        "{}": 123.0
                      }}"#, measurement);
            let time = "2013-06-22T17:03:14.453+02:00";
            let expected_output = format!(r#"{{
                  "time": "{}",
                  "{}": {{
                  "{}": {{
                       "value": 123.0
                      }}
                   }},
                  "type": "ThinEdgeMeasurement"
                }}"#, time, measurement, measurement);

        let entity = CloudEntityMetadata::new("foo".into(), EntityMetadata::main_device(None));
        let output = from_thin_edge_json(input.as_str(), &entity, "").unwrap();
        assert_eq!(
            expected_output.split_whitespace().collect::<String>(),
            output
                .split_whitespace()
                .collect::<String>()
        );
        }
    }

    #[test_case(
    "child1",
    r#"{"temperature": 23.0}"#,
    json!({
        "externalSource": {"externalId": "child1","type": "c8y_Serial",},
        "time": "2021-04-08T00:00:00+05:00",
        "temperature": {"temperature": {"value": 23.0}},
        "type": "ThinEdgeMeasurement"
    })
    ;"child device single value thin-edge json translation")]
    #[test_case(
    "child2",
    r#"{"temperature": 23.0, "pressure": 220.0}"#,
    json!({
        "externalSource": {"externalId": "child2","type": "c8y_Serial",},
        "time": "2021-04-08T00:00:00+05:00",
        "temperature": {"temperature": {"value": 23.0}},
        "pressure": {"pressure": {"value": 220.0}},
        "type": "ThinEdgeMeasurement"
    })
    ;"child device multiple values thin-edge json translation")]
    #[test_case(
    "child3",
    r#"{"temperature": 23.0, "time": "2021-04-23T19:00:00+05:00"}"#,
    json!({
        "externalSource": {"externalId": "child3","type": "c8y_Serial",},
        "time": "2021-04-23T19:00:00+05:00",
        "temperature": {"temperature": {"value": 23.0}},
        "type": "ThinEdgeMeasurement"
    })
    ;"child device single value with timestamp thin-edge json translation")]
    fn check_value_translation_for_child_device(
        child_id: &str,
        thin_edge_json: &str,
        expected_output: Value,
    ) {
        let timestamp = datetime!(2021-04-08 0:00:0 +05:00);
        let entity = CloudEntityMetadata::new(
            child_id.into(),
            EntityMetadata::child_device(child_id.to_string()).unwrap(),
        );
        let output = from_thin_edge_json_with_timestamp(thin_edge_json, timestamp, &entity, "");
        assert_json_eq!(
            serde_json::from_str::<serde_json::Value>(output.unwrap().as_str()).unwrap(),
            expected_output
        );
    }
}
