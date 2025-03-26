use std::collections::HashMap;
use serde_json::Value;
use mprisence::metadata::MetadataSource;
use mpris::Metadata;

use mprisence:: utils::{format_duration, format_track_number, format_audio_channels, format_bitrate, format_sample_rate, format_bit_depth, normalize_player_identity};

fn create_extended_mpris_metadata() -> Metadata {
    let mut data = HashMap::new();
    
    // String fields (both MPRIS and Lofty)
    data.insert("xesam:title".to_string(), "Test Title".into());
    data.insert("xesam:album".to_string(), "Test Album".into());
    data.insert("xesam:composer".to_string(), "Test Composer".into());
    data.insert("xesam:lyricist".to_string(), "Test Lyricist".into());
    data.insert("xesam:conductor".to_string(), "Test Conductor".into());
    data.insert("xesam:remixer".to_string(), "Test Remixer".into());
    data.insert("xesam:language".to_string(), "eng".into());
    data.insert("xesam:encodedBy".to_string(), "Test Encoder".into());
    data.insert("xesam:encoderSettings".to_string(), "LAME 3.100".into());
    data.insert("xesam:comment".to_string(), "Test Comment".into());
    
    // Array fields
    data.insert("xesam:genre".to_string(), vec!["Rock".into(), "Metal".into()].into());
    data.insert("xesam:artist".to_string(), vec!["Artist 1".into(), "Artist 2".into()].into());
    
    // Numeric fields (both MPRIS and Lofty)
    data.insert("xesam:trackNumber".to_string(), "1".into());
    data.insert("xesam:trackTotal".to_string(), "12".into());
    data.insert("xesam:discNumber".to_string(), "2".into());
    data.insert("xesam:discTotal".to_string(), "3".into());
    data.insert("xesam:year".to_string(), "2024".into());
    
    // MPRIS-only string fields
    data.insert("xesam:copyright".to_string(), "Test Copyright".into());
    data.insert("xesam:publisher".to_string(), "Test Publisher".into());
    data.insert("xesam:movement".to_string(), "Test Movement".into());
    
    // MPRIS-only u32 fields
    data.insert("xesam:movementNumber".to_string(), "3".into());
    data.insert("xesam:movementTotal".to_string(), "4".into());
    data.insert("xesam:useCount".to_string(), "42".into());

    // Length in microseconds (3:30 = 210 seconds = 210_000_000 microseconds)
    let length_value = (210_000_000i64).into();
    data.insert("mpris:length".to_string(), length_value);

    // URL and art URL
    data.insert("xesam:url".to_string(), "file:///music/test.mp3".into());
    data.insert("mpris:artUrl".to_string(), "file:///covers/test.jpg".into());

    Metadata::from(data)
}

#[test]
fn test_string_fields() {
    let metadata = MetadataSource::from_mpris(create_extended_mpris_metadata());
    let media = metadata.to_media_metadata();
    
    // Test string fields that exist in both MPRIS and Lofty
    assert_eq!(media.title, Some("Test Title".to_string()));
    assert_eq!(media.album, Some("Test Album".to_string()));
    assert_eq!(media.composer, Some("Test Composer".to_string()));
    assert_eq!(media.lyricist, Some("Test Lyricist".to_string()));
    assert_eq!(media.conductor, Some("Test Conductor".to_string()));
    assert_eq!(media.remixer, Some("Test Remixer".to_string()));
}

#[test]
fn test_array_fields() {
    let metadata = MetadataSource::from_mpris(create_extended_mpris_metadata());
    let media = metadata.to_media_metadata();
    
    // Test array fields
    assert_eq!(media.genres, vec!["Rock".to_string(), "Metal".to_string()]);
    assert_eq!(media.genre_display, Some("Rock, Metal".to_string()));
    
    assert_eq!(media.artists, vec!["Artist 1".to_string(), "Artist 2".to_string()]);
    assert_eq!(media.artist_display, Some("Artist 1, Artist 2".to_string()));
}

#[test]
fn test_numeric_fields() {
    let metadata = MetadataSource::from_mpris(create_extended_mpris_metadata());
    let media = metadata.to_media_metadata();
    
    // Test numeric fields that exist in both MPRIS and Lofty
    assert_eq!(media.track_number, Some(1));
    assert_eq!(media.track_total, Some(12));
    assert_eq!(media.track_display, Some("1/12".to_string()));
    
    assert_eq!(media.disc_number, Some(2));
    assert_eq!(media.disc_total, Some(3));
    assert_eq!(media.disc_display, Some("2/3".to_string()));
    
    assert_eq!(media.year, Some("2024".to_string()));
}

#[test]
fn test_mpris_only_fields() {
    let metadata = MetadataSource::from_mpris(create_extended_mpris_metadata());
    let media = metadata.to_media_metadata();
    
    // Test MPRIS-only string fields
    assert_eq!(media.copyright, Some("Test Copyright".to_string()));
    assert_eq!(media.publisher, Some("Test Publisher".to_string()));
    assert_eq!(media.movement, Some("Test Movement".to_string()));
    
    // Test MPRIS-only u32 fields
    assert_eq!(media.movement_number, Some(3));
    assert_eq!(media.movement_total, Some(4));
    assert_eq!(media.movement_display, Some("3/4".to_string()));
    assert_eq!(media.use_count, Some(42));
}

#[test]
fn test_missing_fields() {
    let metadata = Metadata::new("/test/1");  // Create empty metadata
    let metadata = MetadataSource::from_mpris(metadata);
    let media = metadata.to_media_metadata();
    
    // Test that missing fields return None
    assert_eq!(media.title, None);
    assert!(media.genres.is_empty());
    assert_eq!(media.track_number, None);
    assert_eq!(media.movement_number, None);
    assert_eq!(media.movement_display, None);
}

#[test]
fn test_invalid_numeric_fields() {
    let mut data = HashMap::new();
    
    // Add invalid numeric values as strings
    data.insert("xesam:trackNumber".to_string(), "invalid".into());
    data.insert("xesam:movementNumber".to_string(), "not_a_number".into());
    
    let metadata = Metadata::from(data);
    let metadata = MetadataSource::from_mpris(metadata);
    let media = metadata.to_media_metadata();
    
    // Test that invalid numeric fields return None
    assert_eq!(media.track_number, None);
    assert_eq!(media.movement_number, None);
    assert_eq!(media.movement_display, None);
}

#[test]
fn test_metadata_extraction() {
    let mpris_metadata = create_extended_mpris_metadata();
    let metadata_source = MetadataSource::from_mpris(mpris_metadata);
    
    // Test basic metadata extraction
    assert_eq!(metadata_source.title(), Some("Test Title".to_string()));
    assert_eq!(metadata_source.album(), Some("Test Album".to_string()));
    assert_eq!(metadata_source.artists().unwrap(), vec!["Artist 1".to_string(), "Artist 2".to_string()]);
    assert_eq!(metadata_source.track_number(), Some(1));
    assert_eq!(metadata_source.genres().unwrap(), vec!["Rock".to_string(), "Metal".to_string()]);
    
    // Test duration extraction
    let length = metadata_source.length().unwrap();
    assert_eq!(length.as_secs(), 210); // 3:30 = 210 seconds
}

#[test]
fn test_to_media_metadata_conversion() {
    let mpris_metadata = create_extended_mpris_metadata();
    let metadata_source = MetadataSource::from_mpris(mpris_metadata);
    let media_metadata = metadata_source.to_media_metadata();
    
    // Check that the MediaMetadata has all expected fields
    assert_eq!(media_metadata.title, Some("Test Title".to_string()));
    assert_eq!(media_metadata.album, Some("Test Album".to_string()));
    assert_eq!(media_metadata.artists, vec!["Artist 1".to_string(), "Artist 2".to_string()]);
    assert_eq!(media_metadata.artist_display, Some("Artist 1, Artist 2".to_string()));
    assert_eq!(media_metadata.track_number, Some(1));
    assert_eq!(media_metadata.genres, vec!["Rock".to_string(), "Metal".to_string()]);
    assert_eq!(media_metadata.genre_display, Some("Rock, Metal".to_string()));
    
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
    raw_data.insert("xesam:title".to_string(), Value::String("Test Title".to_string()));
    raw_data.insert("xesam:artist".to_string(), Value::Array(vec![Value::String("Artist 1".to_string()), Value::String("Artist 2".to_string())]));
    raw_data.insert("xesam:album".to_string(), Value::String("Test Album".to_string()));
    raw_data.insert("xesam:trackNumber".to_string(), Value::String("1".to_string()));
    
    // Verify formatting utilities with same data
    assert_eq!(format_track_number(1, Some(12)), "1/12");
    assert_eq!(format_duration(225), "03:45"); // 3:45 = 225 seconds
    
    // Create metadata from the raw data
    let metadata_value = serde_json::to_value(raw_data).unwrap();
    
    // Verify we can access expected fields in the JSON structure
    if let Value::Object(obj) = &metadata_value {
        if let Some(Value::String(title)) = obj.get("xesam:title") {
            assert_eq!(title, "Test Title");
        } else {
            panic!("Title not found or not a string");
        }
        
        if let Some(Value::Array(artists)) = obj.get("xesam:artist") {
            if let Some(Value::String(artist)) = artists.first() {
                assert_eq!(artist, "Artist 1");
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

#[test]
fn test_extended_metadata_fields() {
    let mpris_metadata = create_extended_mpris_metadata();
    let metadata_source = MetadataSource::from_mpris(mpris_metadata);
    let media_metadata = metadata_source.to_media_metadata();
    
    // Test composer and performers
    assert_eq!(media_metadata.composer, Some("Test Composer".to_string()));
    assert_eq!(media_metadata.lyricist, Some("Test Lyricist".to_string()));
    assert_eq!(media_metadata.conductor, Some("Test Conductor".to_string()));
    assert_eq!(media_metadata.remixer, Some("Test Remixer".to_string()));
    
    // Test technical metadata
    assert_eq!(media_metadata.language, Some("eng".to_string()));
    assert_eq!(media_metadata.encoded_by, Some("Test Encoder".to_string()));
    assert_eq!(media_metadata.encoder_settings, Some("LAME 3.100".to_string()));
    
    // Test additional metadata
    assert_eq!(media_metadata.comment, Some("Test Comment".to_string()));
    assert_eq!(media_metadata.copyright, Some("Test Copyright".to_string()));
    assert_eq!(media_metadata.publisher, Some("Test Publisher".to_string()));
    
    // Test classical music metadata
    assert_eq!(media_metadata.movement, Some("Test Movement".to_string()));
    assert_eq!(media_metadata.movement_number, Some(3));
    assert_eq!(media_metadata.movement_total, Some(4));
    assert_eq!(media_metadata.movement_display, Some("3/4".to_string()));
    
    // Test usage metadata
    assert_eq!(media_metadata.use_count, Some(42));
} 