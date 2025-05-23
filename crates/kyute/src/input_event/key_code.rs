use winit::keyboard::{KeyCode, NamedKey, PhysicalKey};

/// Converts winit key event to keyboard_types Key+Code.
//
// Why? Because I don't like nested enums for physical_key and logical_key. Also keyboard_types follows
// a W3C spec, which is always a good thing.
pub(crate) fn key_event_to_key_code(input: &winit::event::KeyEvent) -> (keyboard_types::Key, keyboard_types::Code) {
    use keyboard_types::{Code, Key};
    let code = match input.physical_key {
        PhysicalKey::Code(code) => match code {
            KeyCode::Backquote => Code::Backquote,
            KeyCode::Backslash => Code::Backslash,
            KeyCode::BracketLeft => Code::BracketLeft,
            KeyCode::BracketRight => Code::BracketRight,
            KeyCode::Comma => Code::Comma,
            KeyCode::Digit0 => Code::Digit0,
            KeyCode::Digit1 => Code::Digit1,
            KeyCode::Digit2 => Code::Digit2,
            KeyCode::Digit3 => Code::Digit3,
            KeyCode::Digit4 => Code::Digit4,
            KeyCode::Digit5 => Code::Digit5,
            KeyCode::Digit6 => Code::Digit6,
            KeyCode::Digit7 => Code::Digit7,
            KeyCode::Digit8 => Code::Digit8,
            KeyCode::Digit9 => Code::Digit9,
            KeyCode::Equal => Code::Equal,
            KeyCode::IntlBackslash => Code::IntlBackslash,
            KeyCode::IntlRo => Code::IntlRo,
            KeyCode::IntlYen => Code::IntlYen,
            KeyCode::KeyA => Code::KeyA,
            KeyCode::KeyB => Code::KeyB,
            KeyCode::KeyC => Code::KeyC,
            KeyCode::KeyD => Code::KeyD,
            KeyCode::KeyE => Code::KeyE,
            KeyCode::KeyF => Code::KeyF,
            KeyCode::KeyG => Code::KeyG,
            KeyCode::KeyH => Code::KeyH,
            KeyCode::KeyI => Code::KeyI,
            KeyCode::KeyJ => Code::KeyJ,
            KeyCode::KeyK => Code::KeyK,
            KeyCode::KeyL => Code::KeyL,
            KeyCode::KeyM => Code::KeyM,
            KeyCode::KeyN => Code::KeyN,
            KeyCode::KeyO => Code::KeyO,
            KeyCode::KeyP => Code::KeyP,
            KeyCode::KeyQ => Code::KeyQ,
            KeyCode::KeyR => Code::KeyR,
            KeyCode::KeyS => Code::KeyS,
            KeyCode::KeyT => Code::KeyT,
            KeyCode::KeyU => Code::KeyU,
            KeyCode::KeyV => Code::KeyV,
            KeyCode::KeyW => Code::KeyW,
            KeyCode::KeyX => Code::KeyX,
            KeyCode::KeyY => Code::KeyY,
            KeyCode::KeyZ => Code::KeyZ,
            KeyCode::Minus => Code::Minus,
            KeyCode::Period => Code::Period,
            KeyCode::Quote => Code::Quote,
            KeyCode::Semicolon => Code::Semicolon,
            KeyCode::Slash => Code::Slash,
            KeyCode::AltLeft => Code::AltLeft,
            KeyCode::AltRight => Code::AltRight,
            KeyCode::Backspace => Code::Backspace,
            KeyCode::CapsLock => Code::CapsLock,
            KeyCode::ContextMenu => Code::ContextMenu,
            KeyCode::ControlLeft => Code::ControlLeft,
            KeyCode::ControlRight => Code::ControlRight,
            KeyCode::Enter => Code::Enter,
            KeyCode::SuperLeft => Code::Super, // FIXME
            KeyCode::SuperRight => Code::Super,
            KeyCode::ShiftLeft => Code::ShiftLeft,
            KeyCode::ShiftRight => Code::ShiftRight,
            KeyCode::Space => Code::Space,
            KeyCode::Tab => Code::Tab,
            KeyCode::Convert => Code::Convert,
            KeyCode::KanaMode => Code::KanaMode,
            KeyCode::Lang1 => Code::Lang1,
            KeyCode::Lang2 => Code::Lang2,
            KeyCode::Lang3 => Code::Lang3,
            KeyCode::Lang4 => Code::Lang4,
            KeyCode::Lang5 => Code::Lang5,
            KeyCode::NonConvert => Code::NonConvert,
            KeyCode::Delete => Code::Delete,
            KeyCode::End => Code::End,
            KeyCode::Help => Code::Help,
            KeyCode::Home => Code::Home,
            KeyCode::Insert => Code::Insert,
            KeyCode::PageDown => Code::PageDown,
            KeyCode::PageUp => Code::PageUp,
            KeyCode::ArrowDown => Code::ArrowDown,
            KeyCode::ArrowLeft => Code::ArrowLeft,
            KeyCode::ArrowRight => Code::ArrowRight,
            KeyCode::ArrowUp => Code::ArrowUp,
            KeyCode::NumLock => Code::NumLock,
            KeyCode::Numpad0 => Code::Numpad0,
            KeyCode::Numpad1 => Code::Numpad1,
            KeyCode::Numpad2 => Code::Numpad2,
            KeyCode::Numpad3 => Code::Numpad3,
            KeyCode::Numpad4 => Code::Numpad4,
            KeyCode::Numpad5 => Code::Numpad5,
            KeyCode::Numpad6 => Code::Numpad6,
            KeyCode::Numpad7 => Code::Numpad7,
            KeyCode::Numpad8 => Code::Numpad8,
            KeyCode::Numpad9 => Code::Numpad9,
            KeyCode::NumpadAdd => Code::NumpadAdd,
            KeyCode::NumpadBackspace => Code::NumpadBackspace,
            KeyCode::NumpadClear => Code::NumpadClear,
            KeyCode::NumpadClearEntry => Code::NumpadClearEntry,
            KeyCode::NumpadComma => Code::NumpadComma,
            KeyCode::NumpadDecimal => Code::NumpadDecimal,
            KeyCode::NumpadDivide => Code::NumpadDivide,
            KeyCode::NumpadEnter => Code::NumpadEnter,
            KeyCode::NumpadEqual => Code::NumpadEqual,
            KeyCode::NumpadHash => Code::NumpadHash,
            KeyCode::NumpadMemoryAdd => Code::NumpadMemoryAdd,
            KeyCode::NumpadMemoryClear => Code::NumpadMemoryClear,
            KeyCode::NumpadMemoryRecall => Code::NumpadMemoryRecall,
            KeyCode::NumpadMemoryStore => Code::NumpadMemoryStore,
            KeyCode::NumpadMemorySubtract => Code::NumpadMemorySubtract,
            KeyCode::NumpadMultiply => Code::NumpadMultiply,
            KeyCode::NumpadParenLeft => Code::NumpadParenLeft,
            KeyCode::NumpadParenRight => Code::NumpadParenRight,
            KeyCode::NumpadStar => Code::NumpadStar,
            KeyCode::NumpadSubtract => Code::NumpadSubtract,
            KeyCode::Escape => Code::Escape,
            KeyCode::Fn => Code::Fn,
            KeyCode::FnLock => Code::FnLock,
            KeyCode::PrintScreen => Code::PrintScreen,
            KeyCode::ScrollLock => Code::ScrollLock,
            KeyCode::Pause => Code::Pause,
            KeyCode::BrowserBack => Code::BrowserBack,
            KeyCode::BrowserFavorites => Code::BrowserFavorites,
            KeyCode::BrowserForward => Code::BrowserForward,
            KeyCode::BrowserHome => Code::BrowserHome,
            KeyCode::BrowserRefresh => Code::BrowserRefresh,
            KeyCode::BrowserSearch => Code::BrowserSearch,
            KeyCode::BrowserStop => Code::BrowserStop,
            KeyCode::Eject => Code::Eject,
            KeyCode::LaunchApp1 => Code::LaunchApp1,
            KeyCode::LaunchApp2 => Code::LaunchApp2,
            KeyCode::LaunchMail => Code::LaunchMail,
            KeyCode::MediaPlayPause => Code::MediaPlayPause,
            KeyCode::MediaSelect => Code::MediaSelect,
            KeyCode::MediaStop => Code::MediaStop,
            KeyCode::MediaTrackNext => Code::MediaTrackNext,
            KeyCode::MediaTrackPrevious => Code::MediaTrackPrevious,
            KeyCode::Power => Code::Power,
            KeyCode::Sleep => Code::Sleep,
            KeyCode::AudioVolumeDown => Code::AudioVolumeDown,
            KeyCode::AudioVolumeMute => Code::AudioVolumeMute,
            KeyCode::AudioVolumeUp => Code::AudioVolumeUp,
            KeyCode::WakeUp => Code::WakeUp,
            KeyCode::Meta => Code::Super,
            KeyCode::Hyper => Code::Hyper,
            KeyCode::Turbo => Code::Turbo,
            KeyCode::Abort => Code::Abort,
            KeyCode::Resume => Code::Resume,
            KeyCode::Suspend => Code::Suspend,
            KeyCode::Again => Code::Again,
            KeyCode::Copy => Code::Copy,
            KeyCode::Cut => Code::Cut,
            KeyCode::Find => Code::Find,
            KeyCode::Open => Code::Open,
            KeyCode::Paste => Code::Paste,
            KeyCode::Props => Code::Props,
            KeyCode::Select => Code::Select,
            KeyCode::Undo => Code::Undo,
            KeyCode::Hiragana => Code::Hiragana,
            KeyCode::Katakana => Code::Katakana,
            KeyCode::F1 => Code::F1,
            KeyCode::F2 => Code::F2,
            KeyCode::F3 => Code::F3,
            KeyCode::F4 => Code::F4,
            KeyCode::F5 => Code::F5,
            KeyCode::F6 => Code::F6,
            KeyCode::F7 => Code::F7,
            KeyCode::F8 => Code::F8,
            KeyCode::F9 => Code::F9,
            KeyCode::F10 => Code::F10,
            KeyCode::F11 => Code::F11,
            KeyCode::F12 => Code::F12,
            KeyCode::F13 => Code::F13,
            KeyCode::F14 => Code::F14,
            KeyCode::F15 => Code::F15,
            KeyCode::F16 => Code::F16,
            KeyCode::F17 => Code::F17,
            KeyCode::F18 => Code::F18,
            KeyCode::F19 => Code::F19,
            KeyCode::F20 => Code::F20,
            KeyCode::F21 => Code::F21,
            KeyCode::F22 => Code::F22,
            KeyCode::F23 => Code::F23,
            KeyCode::F24 => Code::F24,
            KeyCode::F25 => Code::F25,
            KeyCode::F26 => Code::F26,
            KeyCode::F27 => Code::F27,
            KeyCode::F28 => Code::F28,
            KeyCode::F29 => Code::F29,
            KeyCode::F30 => Code::F30,
            KeyCode::F31 => Code::F31,
            KeyCode::F32 => Code::F32,
            KeyCode::F33 => Code::F33,
            KeyCode::F34 => Code::F34,
            KeyCode::F35 => Code::F35,
            _ => Code::Unidentified,
        },
        PhysicalKey::Unidentified(_) => Code::Unidentified,
    };

    let key = match &input.logical_key {
        winit::keyboard::Key::Named(nk) => match nk {
            NamedKey::Alt => Key::Alt,
            NamedKey::AltGraph => Key::AltGraph,
            NamedKey::CapsLock => Key::CapsLock,
            NamedKey::Control => Key::Control,
            NamedKey::Fn => Key::Fn,
            NamedKey::FnLock => Key::FnLock,
            NamedKey::NumLock => Key::NumLock,
            NamedKey::ScrollLock => Key::ScrollLock,
            NamedKey::Shift => Key::Shift,
            NamedKey::Symbol => Key::Symbol,
            NamedKey::SymbolLock => Key::SymbolLock,
            NamedKey::Meta => Key::Meta,
            NamedKey::Hyper => Key::Hyper,
            NamedKey::Super => Key::Super,
            NamedKey::Enter => Key::Enter,
            NamedKey::Tab => Key::Tab,
            NamedKey::Space => Key::Character(" ".to_string()),
            NamedKey::ArrowDown => Key::ArrowDown,
            NamedKey::ArrowLeft => Key::ArrowLeft,
            NamedKey::ArrowRight => Key::ArrowRight,
            NamedKey::ArrowUp => Key::ArrowUp,
            NamedKey::End => Key::End,
            NamedKey::Home => Key::Home,
            NamedKey::PageDown => Key::PageDown,
            NamedKey::PageUp => Key::PageUp,
            NamedKey::Backspace => Key::Backspace,
            NamedKey::Clear => Key::Clear,
            NamedKey::Copy => Key::Copy,
            NamedKey::CrSel => Key::CrSel,
            NamedKey::Cut => Key::Cut,
            NamedKey::Delete => Key::Delete,
            NamedKey::EraseEof => Key::EraseEof,
            NamedKey::ExSel => Key::ExSel,
            NamedKey::Insert => Key::Insert,
            NamedKey::Paste => Key::Paste,
            NamedKey::Redo => Key::Redo,
            NamedKey::Undo => Key::Undo,
            NamedKey::Accept => Key::Accept,
            NamedKey::Again => Key::Again,
            NamedKey::Attn => Key::Attn,
            NamedKey::Cancel => Key::Cancel,
            NamedKey::ContextMenu => Key::ContextMenu,
            NamedKey::Escape => Key::Escape,
            NamedKey::Execute => Key::Execute,
            NamedKey::Find => Key::Find,
            NamedKey::Help => Key::Help,
            NamedKey::Pause => Key::Pause,
            NamedKey::Play => Key::Play,
            NamedKey::Props => Key::Props,
            NamedKey::Select => Key::Select,
            NamedKey::ZoomIn => Key::ZoomIn,
            NamedKey::ZoomOut => Key::ZoomOut,
            NamedKey::BrightnessDown => Key::BrightnessDown,
            NamedKey::BrightnessUp => Key::BrightnessUp,
            NamedKey::Eject => Key::Eject,
            NamedKey::LogOff => Key::LogOff,
            NamedKey::Power => Key::Power,
            NamedKey::PowerOff => Key::PowerOff,
            NamedKey::PrintScreen => Key::PrintScreen,
            NamedKey::Hibernate => Key::Hibernate,
            NamedKey::Standby => Key::Standby,
            NamedKey::WakeUp => Key::WakeUp,
            NamedKey::AllCandidates => Key::AllCandidates,
            NamedKey::Alphanumeric => Key::Alphanumeric,
            NamedKey::CodeInput => Key::CodeInput,
            NamedKey::Compose => Key::Compose,
            NamedKey::Convert => Key::Convert,
            NamedKey::FinalMode => Key::FinalMode,
            NamedKey::GroupFirst => Key::GroupFirst,
            NamedKey::GroupLast => Key::GroupLast,
            NamedKey::GroupNext => Key::GroupNext,
            NamedKey::GroupPrevious => Key::GroupPrevious,
            NamedKey::ModeChange => Key::ModeChange,
            NamedKey::NextCandidate => Key::NextCandidate,
            NamedKey::NonConvert => Key::NonConvert,
            NamedKey::PreviousCandidate => Key::PreviousCandidate,
            NamedKey::Process => Key::Process,
            NamedKey::SingleCandidate => Key::SingleCandidate,
            NamedKey::HangulMode => Key::HangulMode,
            NamedKey::HanjaMode => Key::HanjaMode,
            NamedKey::JunjaMode => Key::JunjaMode,
            NamedKey::Eisu => Key::Eisu,
            NamedKey::Hankaku => Key::Hankaku,
            NamedKey::Hiragana => Key::Hiragana,
            NamedKey::HiraganaKatakana => Key::HiraganaKatakana,
            NamedKey::KanaMode => Key::KanaMode,
            NamedKey::KanjiMode => Key::KanjiMode,
            NamedKey::Katakana => Key::Katakana,
            NamedKey::Romaji => Key::Romaji,
            NamedKey::Zenkaku => Key::Zenkaku,
            NamedKey::ZenkakuHankaku => Key::ZenkakuHankaku,
            NamedKey::Soft1 => Key::Soft1,
            NamedKey::Soft2 => Key::Soft2,
            NamedKey::Soft3 => Key::Soft3,
            NamedKey::Soft4 => Key::Soft4,
            NamedKey::ChannelDown => Key::ChannelDown,
            NamedKey::ChannelUp => Key::ChannelUp,
            NamedKey::Close => Key::Close,
            NamedKey::MailForward => Key::MailForward,
            NamedKey::MailReply => Key::MailReply,
            NamedKey::MailSend => Key::MailSend,
            NamedKey::MediaClose => Key::MediaClose,
            NamedKey::MediaFastForward => Key::MediaFastForward,
            NamedKey::MediaPause => Key::MediaPause,
            NamedKey::MediaPlay => Key::MediaPlay,
            NamedKey::MediaPlayPause => Key::MediaPlayPause,
            NamedKey::MediaRecord => Key::MediaRecord,
            NamedKey::MediaRewind => Key::MediaRewind,
            NamedKey::MediaStop => Key::MediaStop,
            NamedKey::MediaTrackNext => Key::MediaTrackNext,
            NamedKey::MediaTrackPrevious => Key::MediaTrackPrevious,
            NamedKey::New => Key::New,
            NamedKey::Open => Key::Open,
            NamedKey::Print => Key::Print,
            NamedKey::Save => Key::Save,
            NamedKey::SpellCheck => Key::SpellCheck,
            NamedKey::Key11 => Key::Key11,
            NamedKey::Key12 => Key::Key12,
            NamedKey::AudioBalanceLeft => Key::AudioBalanceLeft,
            NamedKey::AudioBalanceRight => Key::AudioBalanceRight,
            NamedKey::AudioBassBoostDown => Key::AudioBassBoostDown,
            NamedKey::AudioBassBoostToggle => Key::AudioBassBoostToggle,
            NamedKey::AudioBassBoostUp => Key::AudioBassBoostUp,
            NamedKey::AudioFaderFront => Key::AudioFaderFront,
            NamedKey::AudioFaderRear => Key::AudioFaderRear,
            NamedKey::AudioSurroundModeNext => Key::AudioSurroundModeNext,
            NamedKey::AudioTrebleDown => Key::AudioTrebleDown,
            NamedKey::AudioTrebleUp => Key::AudioTrebleUp,
            NamedKey::AudioVolumeDown => Key::AudioVolumeDown,
            NamedKey::AudioVolumeUp => Key::AudioVolumeUp,
            NamedKey::AudioVolumeMute => Key::AudioVolumeMute,
            NamedKey::MicrophoneToggle => Key::MicrophoneToggle,
            NamedKey::MicrophoneVolumeDown => Key::MicrophoneVolumeDown,
            NamedKey::MicrophoneVolumeUp => Key::MicrophoneVolumeUp,
            NamedKey::MicrophoneVolumeMute => Key::MicrophoneVolumeMute,
            NamedKey::SpeechCorrectionList => Key::SpeechCorrectionList,
            NamedKey::SpeechInputToggle => Key::SpeechInputToggle,
            NamedKey::LaunchApplication1 => Key::LaunchApplication1,
            NamedKey::LaunchApplication2 => Key::LaunchApplication2,
            NamedKey::LaunchCalendar => Key::LaunchCalendar,
            NamedKey::LaunchContacts => Key::LaunchContacts,
            NamedKey::LaunchMail => Key::LaunchMail,
            NamedKey::LaunchMediaPlayer => Key::LaunchMediaPlayer,
            NamedKey::LaunchMusicPlayer => Key::LaunchMusicPlayer,
            NamedKey::LaunchPhone => Key::LaunchPhone,
            NamedKey::LaunchScreenSaver => Key::LaunchScreenSaver,
            NamedKey::LaunchSpreadsheet => Key::LaunchSpreadsheet,
            NamedKey::LaunchWebBrowser => Key::LaunchWebBrowser,
            NamedKey::LaunchWebCam => Key::LaunchWebCam,
            NamedKey::LaunchWordProcessor => Key::LaunchWordProcessor,
            NamedKey::BrowserBack => Key::BrowserBack,
            NamedKey::BrowserFavorites => Key::BrowserFavorites,
            NamedKey::BrowserForward => Key::BrowserForward,
            NamedKey::BrowserHome => Key::BrowserHome,
            NamedKey::BrowserRefresh => Key::BrowserRefresh,
            NamedKey::BrowserSearch => Key::BrowserSearch,
            NamedKey::BrowserStop => Key::BrowserStop,
            NamedKey::AppSwitch => Key::AppSwitch,
            NamedKey::Call => Key::Call,
            NamedKey::Camera => Key::Camera,
            NamedKey::CameraFocus => Key::CameraFocus,
            NamedKey::EndCall => Key::EndCall,
            NamedKey::GoBack => Key::GoBack,
            NamedKey::GoHome => Key::GoHome,
            NamedKey::HeadsetHook => Key::HeadsetHook,
            NamedKey::LastNumberRedial => Key::LastNumberRedial,
            NamedKey::Notification => Key::Notification,
            NamedKey::MannerMode => Key::MannerMode,
            NamedKey::VoiceDial => Key::VoiceDial,
            NamedKey::TV => Key::TV,
            NamedKey::TV3DMode => Key::TV3DMode,
            NamedKey::TVAntennaCable => Key::TVAntennaCable,
            NamedKey::TVAudioDescription => Key::TVAudioDescription,
            NamedKey::TVAudioDescriptionMixDown => Key::TVAudioDescriptionMixDown,
            NamedKey::TVAudioDescriptionMixUp => Key::TVAudioDescriptionMixUp,
            NamedKey::TVContentsMenu => Key::TVContentsMenu,
            NamedKey::TVDataService => Key::TVDataService,
            NamedKey::TVInput => Key::TVInput,
            NamedKey::TVInputComponent1 => Key::TVInputComponent1,
            NamedKey::TVInputComponent2 => Key::TVInputComponent2,
            NamedKey::TVInputComposite1 => Key::TVInputComposite1,
            NamedKey::TVInputComposite2 => Key::TVInputComposite2,
            NamedKey::TVInputHDMI1 => Key::TVInputHDMI1,
            NamedKey::TVInputHDMI2 => Key::TVInputHDMI2,
            NamedKey::TVInputHDMI3 => Key::TVInputHDMI3,
            NamedKey::TVInputHDMI4 => Key::TVInputHDMI4,
            NamedKey::TVInputVGA1 => Key::TVInputVGA1,
            NamedKey::TVMediaContext => Key::TVMediaContext,
            NamedKey::TVNetwork => Key::TVNetwork,
            NamedKey::TVNumberEntry => Key::TVNumberEntry,
            NamedKey::TVPower => Key::TVPower,
            NamedKey::TVRadioService => Key::TVRadioService,
            NamedKey::TVSatellite => Key::TVSatellite,
            NamedKey::TVSatelliteBS => Key::TVSatelliteBS,
            NamedKey::TVSatelliteCS => Key::TVSatelliteCS,
            NamedKey::TVSatelliteToggle => Key::TVSatelliteToggle,
            NamedKey::TVTerrestrialAnalog => Key::TVTerrestrialAnalog,
            NamedKey::TVTerrestrialDigital => Key::TVTerrestrialDigital,
            NamedKey::TVTimer => Key::TVTimer,
            NamedKey::AVRInput => Key::AVRInput,
            NamedKey::AVRPower => Key::AVRPower,
            NamedKey::ColorF0Red => Key::ColorF0Red,
            NamedKey::ColorF1Green => Key::ColorF1Green,
            NamedKey::ColorF2Yellow => Key::ColorF2Yellow,
            NamedKey::ColorF3Blue => Key::ColorF3Blue,
            NamedKey::ColorF4Grey => Key::ColorF4Grey,
            NamedKey::ColorF5Brown => Key::ColorF5Brown,
            NamedKey::ClosedCaptionToggle => Key::ClosedCaptionToggle,
            NamedKey::Dimmer => Key::Dimmer,
            NamedKey::DisplaySwap => Key::DisplaySwap,
            NamedKey::DVR => Key::DVR,
            NamedKey::Exit => Key::Exit,
            NamedKey::FavoriteClear0 => Key::FavoriteClear0,
            NamedKey::FavoriteClear1 => Key::FavoriteClear1,
            NamedKey::FavoriteClear2 => Key::FavoriteClear2,
            NamedKey::FavoriteClear3 => Key::FavoriteClear3,
            NamedKey::FavoriteRecall0 => Key::FavoriteRecall0,
            NamedKey::FavoriteRecall1 => Key::FavoriteRecall1,
            NamedKey::FavoriteRecall2 => Key::FavoriteRecall2,
            NamedKey::FavoriteRecall3 => Key::FavoriteRecall3,
            NamedKey::FavoriteStore0 => Key::FavoriteStore0,
            NamedKey::FavoriteStore1 => Key::FavoriteStore1,
            NamedKey::FavoriteStore2 => Key::FavoriteStore2,
            NamedKey::FavoriteStore3 => Key::FavoriteStore3,
            NamedKey::Guide => Key::Guide,
            NamedKey::GuideNextDay => Key::GuideNextDay,
            NamedKey::GuidePreviousDay => Key::GuidePreviousDay,
            NamedKey::Info => Key::Info,
            NamedKey::InstantReplay => Key::InstantReplay,
            NamedKey::Link => Key::Link,
            NamedKey::ListProgram => Key::ListProgram,
            NamedKey::LiveContent => Key::LiveContent,
            NamedKey::Lock => Key::Lock,
            NamedKey::MediaApps => Key::MediaApps,
            NamedKey::MediaAudioTrack => Key::MediaAudioTrack,
            NamedKey::MediaLast => Key::MediaLast,
            NamedKey::MediaSkipBackward => Key::MediaSkipBackward,
            NamedKey::MediaSkipForward => Key::MediaSkipForward,
            NamedKey::MediaStepBackward => Key::MediaStepBackward,
            NamedKey::MediaStepForward => Key::MediaStepForward,
            NamedKey::MediaTopMenu => Key::MediaTopMenu,
            NamedKey::NavigateIn => Key::NavigateIn,
            NamedKey::NavigateNext => Key::NavigateNext,
            NamedKey::NavigateOut => Key::NavigateOut,
            NamedKey::NavigatePrevious => Key::NavigatePrevious,
            NamedKey::NextFavoriteChannel => Key::NextFavoriteChannel,
            NamedKey::NextUserProfile => Key::NextUserProfile,
            NamedKey::OnDemand => Key::OnDemand,
            NamedKey::Pairing => Key::Pairing,
            NamedKey::PinPDown => Key::PinPDown,
            NamedKey::PinPMove => Key::PinPMove,
            NamedKey::PinPToggle => Key::PinPToggle,
            NamedKey::PinPUp => Key::PinPUp,
            NamedKey::PlaySpeedDown => Key::PlaySpeedDown,
            NamedKey::PlaySpeedReset => Key::PlaySpeedReset,
            NamedKey::PlaySpeedUp => Key::PlaySpeedUp,
            NamedKey::RandomToggle => Key::RandomToggle,
            NamedKey::RcLowBattery => Key::RcLowBattery,
            NamedKey::RecordSpeedNext => Key::RecordSpeedNext,
            NamedKey::RfBypass => Key::RfBypass,
            NamedKey::ScanChannelsToggle => Key::ScanChannelsToggle,
            NamedKey::ScreenModeNext => Key::ScreenModeNext,
            NamedKey::Settings => Key::Settings,
            NamedKey::SplitScreenToggle => Key::SplitScreenToggle,
            NamedKey::STBInput => Key::STBInput,
            NamedKey::STBPower => Key::STBPower,
            NamedKey::Subtitle => Key::Subtitle,
            NamedKey::Teletext => Key::Teletext,
            NamedKey::VideoModeNext => Key::VideoModeNext,
            NamedKey::Wink => Key::Wink,
            NamedKey::ZoomToggle => Key::ZoomToggle,
            NamedKey::F1 => Key::F1,
            NamedKey::F2 => Key::F2,
            NamedKey::F3 => Key::F3,
            NamedKey::F4 => Key::F4,
            NamedKey::F5 => Key::F5,
            NamedKey::F6 => Key::F6,
            NamedKey::F7 => Key::F7,
            NamedKey::F8 => Key::F8,
            NamedKey::F9 => Key::F9,
            NamedKey::F10 => Key::F10,
            NamedKey::F11 => Key::F11,
            NamedKey::F12 => Key::F12,
            NamedKey::F13 => Key::F13,
            NamedKey::F14 => Key::F14,
            NamedKey::F15 => Key::F15,
            NamedKey::F16 => Key::F16,
            NamedKey::F17 => Key::F17,
            NamedKey::F18 => Key::F18,
            NamedKey::F19 => Key::F19,
            NamedKey::F20 => Key::F20,
            NamedKey::F21 => Key::F21,
            NamedKey::F22 => Key::F22,
            NamedKey::F23 => Key::F23,
            NamedKey::F24 => Key::F24,
            NamedKey::F25 => Key::F25,
            NamedKey::F26 => Key::F26,
            NamedKey::F27 => Key::F27,
            NamedKey::F28 => Key::F28,
            NamedKey::F29 => Key::F29,
            NamedKey::F30 => Key::F30,
            NamedKey::F31 => Key::F31,
            NamedKey::F32 => Key::F32,
            NamedKey::F33 => Key::F33,
            NamedKey::F34 => Key::F34,
            NamedKey::F35 => Key::F35,
            _ => Key::Unidentified,
        },
        winit::keyboard::Key::Character(str) => Key::Character(str.to_string()),
        winit::keyboard::Key::Unidentified(_) => Key::Unidentified,
        winit::keyboard::Key::Dead(_ch) => Key::Unidentified,
    };

    (key, code)
}

/*
pub(crate) fn to_keyboard_type_modifiers(modifiers: winit::event::Modifiers) -> keyboard_types::Modifiers {
    let mut mods = keyboard_types::Modifiers::default();
    if modifiers.state().control_key() {
        mods |= Modifiers::CONTROL;
    }
    if modifiers.state().alt_key() {
        mods |= Modifiers::ALT;
    }
    if modifiers.state().shift_key() {
        mods |= Modifiers::SHIFT;
    }
    if modifiers.state().super_key() {
        mods |= Modifiers::SUPER;
    }
    mods
}
*/
