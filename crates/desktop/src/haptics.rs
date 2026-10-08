use gpui::App;

pub trait HapticsExt {
    fn play_alignment_haptic(&self);
}

impl HapticsExt for App {
    #[cfg(target_os = "macos")]
    fn play_alignment_haptic(&self) {
        use objc2::MainThreadMarker;
        use objc2_app_kit::{
            NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime,
            NSHapticFeedbackPerformer as _,
        };

        if MainThreadMarker::new().is_none() {
            return;
        }

        NSHapticFeedbackManager::defaultPerformer().performFeedbackPattern_performanceTime(
            NSHapticFeedbackPattern::Alignment,
            NSHapticFeedbackPerformanceTime::Now,
        );
    }

    #[cfg(not(target_os = "macos"))]
    fn play_alignment_haptic(&self) {}
}
