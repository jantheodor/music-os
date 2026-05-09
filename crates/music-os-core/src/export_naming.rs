use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportNamingContext {
    Album {
        track_number: u32,
        max_track_number: Option<u32>,
    },
    Loose,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportNamingInput {
    pub artist_name: String,
    pub track_name: String,
    pub extension: Option<String>,
    pub context: ExportNamingContext,
}

pub fn build_export_filename(input: &ExportNamingInput) -> Result<String> {
    let artist_name = clean_filename_component(&input.artist_name, "artist_name")?;
    let track_name = clean_filename_component(&input.track_name, "track_name")?;
    let stem = match input.context {
        ExportNamingContext::Album {
            track_number,
            max_track_number,
        } => {
            if track_number == 0 {
                return Err(anyhow!("track_number must be greater than zero"));
            }
            let highest_track_number = max_track_number.unwrap_or(track_number).max(track_number);
            let width = digit_count(highest_track_number).max(2);
            format!("{track_number:0width$}. {artist_name} - {track_name}")
        }
        ExportNamingContext::Loose => format!("{artist_name} - {track_name}"),
    };

    Ok(match clean_extension(input.extension.as_deref()) {
        Some(extension) => format!("{stem}.{extension}"),
        None => stem,
    })
}

fn clean_filename_component(value: &str, field_name: &str) -> Result<String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = normalized
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            character if character.is_control() => '_',
            character => character,
        })
        .collect::<String>()
        .trim_matches([' ', '.', '_'])
        .to_string();

    if cleaned.is_empty() {
        Err(anyhow!("{field_name} must not be empty"))
    } else {
        Ok(cleaned)
    }
}

fn clean_extension(extension: Option<&str>) -> Option<String> {
    extension
        .map(|extension| extension.trim().trim_start_matches('.'))
        .filter(|extension| !extension.is_empty())
        .map(|extension| {
            extension
                .chars()
                .filter(|character| character.is_ascii_alphanumeric())
                .collect::<String>()
        })
        .filter(|extension| !extension.is_empty())
}

fn digit_count(value: u32) -> usize {
    value.to_string().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_tracks_use_two_digit_track_number_by_default() {
        let filename = build_export_filename(&ExportNamingInput {
            artist_name: "Eminem".to_string(),
            track_name: "Stan".to_string(),
            extension: Some("mp3".to_string()),
            context: ExportNamingContext::Album {
                track_number: 3,
                max_track_number: Some(18),
            },
        })
        .expect("filename");

        assert_eq!(filename, "03. Eminem - Stan.mp3");
    }

    #[test]
    fn album_tracks_expand_width_for_large_albums_or_compilations() {
        let filename = build_export_filename(&ExportNamingInput {
            artist_name: "Various Artists".to_string(),
            track_name: "Long Compilation Track".to_string(),
            extension: Some(".flac".to_string()),
            context: ExportNamingContext::Album {
                track_number: 7,
                max_track_number: Some(120),
            },
        })
        .expect("filename");

        assert_eq!(
            filename,
            "007. Various Artists - Long Compilation Track.flac"
        );
    }

    #[test]
    fn loose_tracks_do_not_include_track_numbers() {
        let filename = build_export_filename(&ExportNamingInput {
            artist_name: "Eminem".to_string(),
            track_name: "Stan".to_string(),
            extension: Some("mp3".to_string()),
            context: ExportNamingContext::Loose,
        })
        .expect("filename");

        assert_eq!(filename, "Eminem - Stan.mp3");
    }

    #[test]
    fn filename_components_are_normalized_for_portable_exports() {
        let filename = build_export_filename(&ExportNamingInput {
            artist_name: "  AC/DC  ".to_string(),
            track_name: "  Hells:Bells?  ".to_string(),
            extension: Some("m p 3".to_string()),
            context: ExportNamingContext::Loose,
        })
        .expect("filename");

        assert_eq!(filename, "AC_DC - Hells_Bells.mp3");
    }

    #[test]
    fn album_track_number_must_be_positive() {
        let error = build_export_filename(&ExportNamingInput {
            artist_name: "Artist".to_string(),
            track_name: "Track".to_string(),
            extension: None,
            context: ExportNamingContext::Album {
                track_number: 0,
                max_track_number: Some(10),
            },
        })
        .expect_err("track number should fail");

        assert!(error.to_string().contains("track_number"));
    }
}
