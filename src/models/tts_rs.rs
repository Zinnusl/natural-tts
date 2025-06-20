use super::{NaturalModelTrait, SynthesizedAudio};
use crate::TtsError;
use std::error::Error;
use tts::Tts;

#[derive(Clone)]
pub struct TtsModel(pub Tts);

impl TtsModel {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let def = Tts::default()?;
        return Ok(Self(def));
    }
}

impl Default for TtsModel {
    fn default() -> Self {
        return Self::new().unwrap();
    }
}

impl NaturalModelTrait for TtsModel {
    type SynthesizeType = f32;
    fn save(&mut self, message: String, path: String) -> Result<(), Box<dyn Error>> {
        Err(TtsError::NotSupported.into())
    }

    fn say(&mut self, message: String) -> Result<(), Box<dyn Error>> {
        let is_speaking = self.0.is_speaking();

        if let Ok(speaking) = is_speaking {
            if speaking {
                return Ok(());
            }
        }

        let _ = self.0.speak(message, false);
        Ok(())
    }

    fn synthesize(
        &mut self,
        message: String,
    ) -> Result<SynthesizedAudio<Self::SynthesizeType>, Box<dyn Error>> {
        Err(TtsError::NotSupported.into())
    }
}
