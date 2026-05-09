use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltInLoudnessProfileId {
    Original,
    AlbumRespect,
    ShuffleSmooth,
    Party,
    Headphones,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackContext {
    Album,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicsPolicy {
    Untouched,
    StronglyPreserved,
    MostlyPreserved,
    EnergeticControlled,
    FatigueReduced,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoudnessProfile {
    pub id: BuiltInLoudnessProfileId,
    pub name: &'static str,
    pub purpose: &'static str,
    pub normalize_loudness: bool,
    pub target_lufs: Option<f64>,
    pub prefer_album_gain: bool,
    pub limiter_allowed: bool,
    pub dynamics_policy: DynamicsPolicy,
    pub default_for_album_playback: bool,
    pub default_for_mixed_playback: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LoudnessAnalysisInput {
    pub integrated_lufs: Option<f64>,
    pub true_peak_db: Option<f64>,
    pub dynamic_range: Option<f64>,
    pub replaygain_track_gain_db: Option<f64>,
    pub replaygain_album_gain_db: Option<f64>,
    pub clipping_risk: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoudnessAdjustmentPlan {
    pub profile_id: BuiltInLoudnessProfileId,
    pub gain_db: Option<f64>,
    pub uses_album_gain: bool,
    pub limiter_enabled: bool,
    pub warning: Option<String>,
}

pub fn built_in_loudness_profiles() -> Vec<LoudnessProfile> {
    vec![
        LoudnessProfile {
            id: BuiltInLoudnessProfileId::Original,
            name: "Original",
            purpose: "Completely untouched playback.",
            normalize_loudness: false,
            target_lufs: None,
            prefer_album_gain: false,
            limiter_allowed: false,
            dynamics_policy: DynamicsPolicy::Untouched,
            default_for_album_playback: false,
            default_for_mixed_playback: false,
        },
        LoudnessProfile {
            id: BuiltInLoudnessProfileId::AlbumRespect,
            name: "Album Respect",
            purpose: "Preserve intended album listening experience.",
            normalize_loudness: true,
            target_lufs: Some(-18.0),
            prefer_album_gain: true,
            limiter_allowed: false,
            dynamics_policy: DynamicsPolicy::StronglyPreserved,
            default_for_album_playback: true,
            default_for_mixed_playback: false,
        },
        LoudnessProfile {
            id: BuiltInLoudnessProfileId::ShuffleSmooth,
            name: "Shuffle Smooth",
            purpose: "Comfortable everyday listening across mixed playlists.",
            normalize_loudness: true,
            target_lufs: Some(-16.0),
            prefer_album_gain: false,
            limiter_allowed: false,
            dynamics_policy: DynamicsPolicy::MostlyPreserved,
            default_for_album_playback: false,
            default_for_mixed_playback: true,
        },
        LoudnessProfile {
            id: BuiltInLoudnessProfileId::Party,
            name: "Party",
            purpose: "Strong loudness consistency and energetic playback.",
            normalize_loudness: true,
            target_lufs: Some(-14.0),
            prefer_album_gain: false,
            limiter_allowed: true,
            dynamics_policy: DynamicsPolicy::EnergeticControlled,
            default_for_album_playback: false,
            default_for_mixed_playback: false,
        },
        LoudnessProfile {
            id: BuiltInLoudnessProfileId::Headphones,
            name: "Headphones",
            purpose: "Pleasant long-form headphone listening.",
            normalize_loudness: true,
            target_lufs: Some(-17.0),
            prefer_album_gain: false,
            limiter_allowed: true,
            dynamics_policy: DynamicsPolicy::FatigueReduced,
            default_for_album_playback: false,
            default_for_mixed_playback: false,
        },
    ]
}

pub fn default_loudness_profile_for_context(context: PlaybackContext) -> BuiltInLoudnessProfileId {
    match context {
        PlaybackContext::Album => BuiltInLoudnessProfileId::AlbumRespect,
        PlaybackContext::Mixed => BuiltInLoudnessProfileId::ShuffleSmooth,
    }
}

pub fn get_built_in_loudness_profile(id: BuiltInLoudnessProfileId) -> LoudnessProfile {
    built_in_loudness_profiles()
        .into_iter()
        .find(|profile| profile.id == id)
        .expect("built-in loudness profile exists")
}

pub fn plan_loudness_adjustment(
    profile_id: BuiltInLoudnessProfileId,
    analysis: LoudnessAnalysisInput,
) -> LoudnessAdjustmentPlan {
    let profile = get_built_in_loudness_profile(profile_id);
    if !profile.normalize_loudness {
        return LoudnessAdjustmentPlan {
            profile_id,
            gain_db: None,
            uses_album_gain: false,
            limiter_enabled: false,
            warning: None,
        };
    }

    let replaygain = if profile.prefer_album_gain {
        analysis
            .replaygain_album_gain_db
            .map(|gain| (gain, true))
            .or_else(|| analysis.replaygain_track_gain_db.map(|gain| (gain, false)))
    } else {
        analysis
            .replaygain_track_gain_db
            .map(|gain| (gain, false))
            .or_else(|| analysis.replaygain_album_gain_db.map(|gain| (gain, true)))
    };

    let (gain_db, uses_album_gain) = match replaygain {
        Some((gain, uses_album_gain)) => (Some(gain), uses_album_gain),
        None => (
            analysis
                .integrated_lufs
                .zip(profile.target_lufs)
                .map(|(integrated_lufs, target_lufs)| target_lufs - integrated_lufs),
            false,
        ),
    };

    let limiter_enabled = profile.limiter_allowed && analysis.clipping_risk;
    LoudnessAdjustmentPlan {
        profile_id,
        gain_db,
        uses_album_gain,
        limiter_enabled,
        warning: if analysis.clipping_risk && !profile.limiter_allowed {
            Some("clipping risk detected; selected profile avoids limiting".to_string())
        } else {
            None
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_profiles_include_curated_defaults() {
        let names = built_in_loudness_profiles()
            .into_iter()
            .map(|profile| profile.name)
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                "Original",
                "Album Respect",
                "Shuffle Smooth",
                "Party",
                "Headphones"
            ]
        );
    }

    #[test]
    fn sensible_default_profiles_are_contextual() {
        assert_eq!(
            default_loudness_profile_for_context(PlaybackContext::Album),
            BuiltInLoudnessProfileId::AlbumRespect
        );
        assert_eq!(
            default_loudness_profile_for_context(PlaybackContext::Mixed),
            BuiltInLoudnessProfileId::ShuffleSmooth
        );
    }

    #[test]
    fn original_profile_produces_no_loudness_adjustment() {
        let plan = plan_loudness_adjustment(
            BuiltInLoudnessProfileId::Original,
            LoudnessAnalysisInput {
                integrated_lufs: Some(-8.0),
                true_peak_db: Some(0.5),
                dynamic_range: Some(5.0),
                replaygain_track_gain_db: Some(-7.0),
                replaygain_album_gain_db: Some(-5.0),
                clipping_risk: true,
            },
        );

        assert_eq!(plan.gain_db, None);
        assert!(!plan.limiter_enabled);
    }

    #[test]
    fn album_respect_prefers_album_gain() {
        let plan = plan_loudness_adjustment(
            BuiltInLoudnessProfileId::AlbumRespect,
            LoudnessAnalysisInput {
                integrated_lufs: Some(-12.0),
                true_peak_db: Some(-0.5),
                dynamic_range: Some(10.0),
                replaygain_track_gain_db: Some(-3.0),
                replaygain_album_gain_db: Some(-1.5),
                clipping_risk: false,
            },
        );

        assert_eq!(plan.gain_db, Some(-1.5));
        assert!(plan.uses_album_gain);
        assert!(!plan.limiter_enabled);
    }

    #[test]
    fn party_profile_allows_limiter_when_clipping_risk_exists() {
        let plan = plan_loudness_adjustment(
            BuiltInLoudnessProfileId::Party,
            LoudnessAnalysisInput {
                integrated_lufs: Some(-20.0),
                true_peak_db: Some(-0.1),
                dynamic_range: Some(6.0),
                replaygain_track_gain_db: None,
                replaygain_album_gain_db: None,
                clipping_risk: true,
            },
        );

        assert_eq!(plan.gain_db, Some(6.0));
        assert!(plan.limiter_enabled);
    }
}
