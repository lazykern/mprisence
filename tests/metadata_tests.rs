use std::collections::HashMap;
use serde_json::Value;

// We need to use the crate name to import from the library
use mprisence::{
    metadata::MetadataSource,
    utils::{format_duration, format_track_number, format_audio_channels, format_bitrate, format_sample_rate, format_bit_depth, normalize_player_identity},
};

// Import the common test module
mod common;
use common::create_test_mpris_metadata;

#[test]
fn test_metadata_extraction() {
    let mpris_metadata = create_test_mpris_metadata();
    let metadata_source = MetadataSource::from_mpris(mpris_metadata);
    
    // Test basic metadata extraction
    assert_eq!(metadata_source.title(), Some("Test Song".to_string()));
    assert_eq!(metadata_source.album(), Some("Test Album".to_string()));
    assert_eq!(metadata_source.artists().unwrap(), vec!["Test Artist".to_string()]);
    assert_eq!(metadata_source.track_number(), Some(7));
    assert_eq!(metadata_source.genres().unwrap(), vec!["Rock".to_string(), "Alternative".to_string()]);
    
    // Test duration extraction
    let length = metadata_source.length().unwrap();
    assert_eq!(length.as_secs(), 210); // 3:30 = 210 seconds
}

#[test]
fn test_to_media_metadata_conversion() {
    let mpris_metadata = create_test_mpris_metadata();
    let metadata_source = MetadataSource::from_mpris(mpris_metadata);
    let media_metadata = metadata_source.to_media_metadata();
    
    // Check that the MediaMetadata has all expected fields
    assert_eq!(media_metadata.title, Some("Test Song".to_string()));
    assert_eq!(media_metadata.album, Some("Test Album".to_string()));
    assert_eq!(media_metadata.artists, vec!["Test Artist".to_string()]);
    assert_eq!(media_metadata.artist_display, Some("Test Artist".to_string()));
    assert_eq!(media_metadata.track_number, Some(7));
    assert_eq!(media_metadata.genres, vec!["Rock".to_string(), "Alternative".to_string()]);
    assert_eq!(media_metadata.genre_display, Some("Rock, Alternative".to_string()));
    
    // Check that formatting is applied correctly
    assert_eq!(media_metadata.duration_display, Some("03:30".to_string()));
}

#[test]
fn test_formatting_functions() {
    // Test duration formatting
    assert_eq!(format_duration(0), "00:00");
    assert_eq!(format_duration(61), "01:01");
    assert_eq!(format_duration(3723), "62:03"); // 1h 2m 3s
    
    // Test track number formatting
    assert_eq!(format_track_number(1, None), "1");
    assert_eq!(format_track_number(5, Some(12)), "5/12");
    
    // Test audio channel formatting
    assert_eq!(format_audio_channels(1), "Mono");
    assert_eq!(format_audio_channels(2), "Stereo");
    assert_eq!(format_audio_channels(6), "6 channels");
    
    // Test other formatting functions
    assert_eq!(format_bitrate(320), "320 kbps");
    assert_eq!(format_sample_rate(44100), "44.1 kHz");
    assert_eq!(format_bit_depth(16), "16-bit");
    
    // Test player name normalization (from metadata_integration.rs)
    assert_eq!(normalize_player_identity("Spotify Player"), "spotify_player");
    assert_eq!(normalize_player_identity("VLC media player"), "vlc_media_player");
    assert_eq!(normalize_player_identity("Firefox"), "firefox");
}

// Add the test from metadata_integration.rs that tests metadata creation from raw data
#[test]
fn test_metadata_from_json() {
    // Create mock metadata similar to what would come from MPRIS
    let mut raw_data = HashMap::new();
    raw_data.insert("xesam:title".to_string(), Value::String("Test Song".to_string()));
    raw_data.insert("xesam:artist".to_string(), Value::Array(vec![Value::String("Test Artist".to_string())]));
    raw_data.insert("xesam:album".to_string(), Value::String("Test Album".to_string()));
    raw_data.insert("xesam:trackNumber".to_string(), Value::String("7".to_string()));
    
    // Verify formatting utilities with same data
    assert_eq!(format_track_number(7, Some(12)), "7/12");
    assert_eq!(format_duration(225), "03:45"); // 3:45 = 225 seconds
    
    // Create metadata from the raw data
    let metadata_value = serde_json::to_value(raw_data).unwrap();
    
    // Verify we can access expected fields in the JSON structure
    if let Value::Object(obj) = &metadata_value {
        if let Some(Value::String(title)) = obj.get("xesam:title") {
            assert_eq!(title, "Test Song");
        } else {
            panic!("Title not found or not a string");
        }
        
        if let Some(Value::Array(artists)) = obj.get("xesam:artist") {
            if let Some(Value::String(artist)) = artists.first() {
                assert_eq!(artist, "Test Artist");
            } else {
                panic!("Artist not found in array or not a string");
            }
        } else {
            panic!("Artist array not found");
        }
    } else {
        panic!("Metadata is not an object");
    }
} 