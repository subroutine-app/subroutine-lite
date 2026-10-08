use gpui::{App, IntoElement, RenderOnce, SharedString, StyleRefinement, Styled, Svg, Window, svg};

pub trait IconNamed {
    fn path(self) -> SharedString;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppIcon {
    Archive,
    Cable,
    CalendarClock,
    CalendarPlus,
    Calendars,
    Clock,
    ClockAlert,
    Filter,
    Home,
    Info,
    ListOrdered,
    ListPlus,
    ListIndentIncrease,
    ListChevronsDownUp,
    ListChevronsUpDown,
    ListChecks,
    LogIn,
    MapPin,
    Pin,
    Check,
    Play,
    Plus,
    Minus,
    MoveUp,
    MoveDown,
    Save,
    ScanEye,
    Search,
    Sliders,

    SquarePen,
    SortVertical,
    Trash,
    Timeline,
    Repeat,
    RotateCcw,
    Close,
    ZoomIn,
    ZoomOut,
    ZoomReset,

    ArrowLeft,
    ArrowRight,
    Calendar,
    ChevronDown,
    ChevronLeft,
    ChevronRight,
    ChevronUp,
    Ellipsis,
    Inbox,
    PanelLeftClose,
    PanelLeftOpen,
    PanelRightClose,
    PanelRightOpen,
    Settings,
}

impl IconNamed for AppIcon {
    fn path(self) -> SharedString {
        match self {
            Self::Archive => "icons/custom/archive.svg",
            Self::Cable => "icons/custom/cable.svg",
            Self::CalendarClock => "icons/custom/calendar-clock.svg",
            Self::Check => "icons/custom/check.svg",
            Self::CalendarPlus => "icons/custom/calendar-plus.svg",
            Self::Calendars => "icons/custom/calendars.svg",
            Self::Clock => "icons/custom/clock.svg",
            Self::ClockAlert => "icons/custom/clock-alert.svg",
            Self::Filter => "icons/filter.svg",
            Self::Home => "icons/custom/house.svg",
            Self::Info => "icons/custom/info.svg",
            Self::ListOrdered => "icons/custom/list-ordered.svg",
            Self::ListPlus => "icons/custom/list-plus.svg",
            Self::ListIndentIncrease => "icons/custom/list-indent-increase.svg",
            Self::ListChevronsDownUp => "icons/custom/list-chevrons-down-up.svg",
            Self::ListChevronsUpDown => "icons/custom/list-chevrons-up-down.svg",
            Self::ListChecks => "icons/custom/list-checks.svg",
            Self::LogIn => "icons/custom/log-in.svg",
            Self::MapPin => "icons/custom/map-pin.svg",
            Self::Pin => "icons/custom/pin.svg",
            Self::Play => "icons/custom/play.svg",
            Self::Plus => "icons/custom/plus.svg",
            Self::Minus => "icons/custom/minus.svg",
            Self::MoveUp => "icons/custom/move-up.svg",
            Self::MoveDown => "icons/custom/move-down.svg",
            Self::Save => "icons/custom/save.svg",
            Self::ScanEye => "icons/custom/scan-eye.svg",
            Self::Search => "icons/search.svg",
            Self::Sliders => "icons/sliders.svg",

            Self::SquarePen => "icons/custom/square-pen.svg",
            Self::SortVertical => "icons/regular/arrows-down-up.svg",
            Self::Trash => "icons/custom/trash.svg",
            Self::Timeline => "icons/custom/timeline.svg",
            Self::RotateCcw => "icons/custom/rotate-ccw.svg",
            Self::Repeat => "icons/custom/repeat.svg",
            Self::Close => "icons/custom/close.svg",
            Self::ZoomIn => "icons/custom/zoom-in.svg",
            Self::ZoomOut => "icons/custom/zoom-out.svg",
            Self::ZoomReset => "icons/custom/zoom-reset.svg",

            Self::ArrowLeft => "icons/arrow-left.svg",
            Self::ArrowRight => "icons/arrow-right.svg",
            Self::Calendar => "icons/calendar.svg",
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::ChevronLeft => "icons/chevron-left.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::ChevronUp => "icons/chevron-up.svg",
            Self::Ellipsis => "icons/ellipsis.svg",
            Self::Inbox => "icons/inbox.svg",
            Self::PanelLeftClose => "icons/panel-left-close.svg",
            Self::PanelLeftOpen => "icons/panel-left-open.svg",
            Self::PanelRightClose => "icons/panel-right-close.svg",
            Self::PanelRightOpen => "icons/panel-right-open.svg",
            Self::Settings => "icons/settings.svg",
        }
        .into()
    }
}

#[derive(IntoElement)]
pub struct Icon {
    svg: Svg,
}

impl Icon {
    pub fn new(icon: impl IconNamed) -> Self {
        Self::from_path(icon.path())
    }

    pub fn from_path(path: impl Into<SharedString>) -> Self {
        Self {
            svg: svg().path(path).flex_none(),
        }
    }

    pub fn has_size(&mut self) -> bool {
        self.style().size.width.is_some()
    }

    pub fn has_color(&mut self) -> bool {
        self.style().text.color.is_some()
    }
}

impl Styled for Icon {
    fn style(&mut self) -> &mut StyleRefinement {
        self.svg.style()
    }
}

impl RenderOnce for Icon {
    fn render(mut self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        if !self.has_color() {
            let inherited = window.text_style().color;
            self.svg = self.svg.text_color(inherited);
        }
        if !self.has_size() {
            self.svg = self.svg.size_4();
        }
        self.svg
    }
}
