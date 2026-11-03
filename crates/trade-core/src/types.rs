use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum PositionSide {
    Long,
    Short,
}

impl std::fmt::Display for PositionSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PositionSide::Long => write!(f, "LONG"),
            PositionSide::Short => write!(f, "SHORT"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum StrategyType {
    VShapeDrop,
    VShapeRip,
    MomentumScalp,
    MeanReversion,
    OpeningSniper,
    DepthPressure,
    VWAPMagnet,
    LiquiditySweep,
    VolumeProfile,
    GapFade,
    TrendPullback,
    Manual,
}

impl std::fmt::Display for StrategyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StrategyType::VShapeDrop => write!(f, "VDrop"),
            StrategyType::VShapeRip => write!(f, "VRip"),
            StrategyType::MomentumScalp => write!(f, "MomSclp"),
            StrategyType::MeanReversion => write!(f, "MeanRev"),
            StrategyType::OpeningSniper => write!(f, "OpenSnip"),
            StrategyType::DepthPressure => write!(f, "OFI"),
            StrategyType::VWAPMagnet => write!(f, "VWAPMag"),
            StrategyType::LiquiditySweep => write!(f, "LiqSwp"),
            StrategyType::VolumeProfile => write!(f, "VolProf"),
            StrategyType::GapFade => write!(f, "GapFade"),
            StrategyType::TrendPullback => write!(f, "Pullbak"),
            StrategyType::Manual => write!(f, "MANUAL"),
        }
    }
}
