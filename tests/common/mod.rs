use std::collections::HashMap;

// Create mock MPRIS metadata for testing
pub fn create_test_mpris_metadata() -> mpris::Metadata {
    let mut data = HashMap::new();
    data.insert("xesam:title".to_string(), "Test Song".into());
    data.insert("xesam:artist".to_string(), vec!["Test Artist".into()].into());
    data.insert("xesam:album".to_string(), "Test Album".into());
    data.insert("xesam:trackNumber".to_string(), "7".into());
    let length_value: mpris::MetadataValue = ((3 * 60 + 30) * 1000000i64).into();
    data.insert("mpris:length".to_string(), length_value);
    data.insert("xesam:url".to_string(), "file:///music/test.mp3".into());
    data.insert("mpris:artUrl".to_string(), "file:///covers/test.jpg".into());
    data.insert("xesam:genre".to_string(), vec!["Rock".into(), "Alternative".into()].into());
    data.insert("xesam:discNumber".to_string(), "1".into());
    
    mpris::Metadata::from(data)
} 