use std::path::PathBuf;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use chronoutil::RelativeDuration;
use clap::{Args, Parser, Subcommand, ValueEnum};
use subroutine_core::SchedulePoint;
use uuid::Uuid;

use crate::{input, resources::Kind};

#[derive(Parser)]
#[command(
    name = "subroutine",
    version,
    about = "Subroutine items and scheduling. JSON on stdout."
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        env = "SUBROUTINE_LITE_CLI_DB",
        conflicts_with = "server",
        value_name = "PATH",
        help = "Local database (default: isolated CLI workspace)"
    )]
    pub db: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        env = "SUBROUTINE_LITE_SERVER_URL",
        conflicts_with = "db",
        value_name = "URL",
        help = "Use a server instead of local storage"
    )]
    pub server: Option<String>,
    #[arg(
        long,
        global = true,
        requires = "server",
        value_name = "PATH",
        help = "Bearer token file; otherwise SUBROUTINE_LITE_ACCESS_TOKEN"
    )]
    pub token_file: Option<PathBuf>,
    #[arg(long, global = true, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=3600), value_name = "SECONDS", help = "HTTP timeout")]
    pub timeout: u64,
    #[arg(long, global = true, value_parser = input::datetime, value_name = "RFC3339", help = "Override the clock; calendar rules use the local timezone")]
    pub now: Option<DateTime<Utc>>,
    #[arg(
        long,
        global = true,
        help = "Preview result and writes without committing"
    )]
    pub dry_run: bool,
    #[arg(
        long,
        global = true,
        conflicts_with = "quiet",
        help = "Indent JSON output"
    )]
    pub pretty: bool,
    #[arg(short, long, global = true, help = "Suppress successful output")]
    pub quiet: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(subcommand, about = "Read, replace, or delete items")]
    Item(ItemCommand),
    #[command(subcommand, about = "Create an item")]
    Add(AddCommand),
    #[command(subcommand, about = "Complete, queue, or pin actions")]
    Action(ActionCommand),
    #[command(subcommand, about = "Run a routine")]
    Routine(RoutineCommand),
    #[command(subcommand, about = "Inspect or reschedule the pipeline")]
    Pipeline(PipelineCommand),
    #[command(about = "Parse natural language without saving")]
    Parse(ParseArgs),
    #[command(subcommand, about = "Export or merge JSON snapshots")]
    Data(DataCommand),
}

impl Command {
    pub fn input_path(&self) -> Option<&std::path::Path> {
        match self {
            Self::Item(ItemCommand::Put { file, .. })
            | Self::Data(DataCommand::Import { file }) => Some(file),
            _ => None,
        }
    }
}

#[derive(Subcommand)]
pub enum ItemCommand {
    #[command(about = "List typed resources")]
    List(ListArgs),
    #[command(about = "Read one resource as JSON")]
    Get { kind: Kind, id: Uuid },
    #[command(about = "Create or replace a full resource from JSON")]
    Put {
        kind: Kind,
        #[arg(
            short,
            long,
            value_name = "PATH",
            help = "JSON file, or - for stdin; accepts item get output"
        )]
        file: PathBuf,
    },
    #[command(about = "Delete one resource")]
    Delete { kind: Kind, id: Uuid },
}

#[derive(Args)]
pub struct ListArgs {
    pub kind: Option<Kind>,
    #[arg(long, value_enum, help = "Filter actions (implies action kind)")]
    pub state: Option<ActionState>,
    #[arg(
        short,
        long,
        value_name = "TEXT",
        help = "Case-insensitive title search"
    )]
    pub search: Option<String>,
    #[arg(long, help = "Maximum number of results")]
    pub limit: Option<usize>,
    #[arg(long, help = "Return only UUIDs")]
    pub ids: bool,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ActionState {
    Open,
    Queued,
    Backlog,
    Completed,
}

#[derive(Args)]
pub struct TextArgs {
    #[arg(value_parser = input::title)]
    pub title: String,

    #[arg(long, help = "Notes or content")]
    pub content: Option<String>,
}

#[derive(Subcommand)]
pub enum AddCommand {
    #[command(about = "Create an action")]
    Action {
        #[command(flatten)]
        text: TextArgs,
        #[arg(long, value_parser = input::schedule_point, value_name = "DATE|RFC3339", conflicts_with = "queue")]
        start: Option<SchedulePoint>,
        #[arg(short, long, value_parser = input::duration, value_name = "30m|PT30M")]
        duration: Option<RelativeDuration>,
        #[arg(long, help = "Place at the next free slot")]
        queue: bool,
        #[arg(long, help = "Pin a timed action")]
        pin: bool,
    },
    #[command(about = "Create a timed event")]
    Event {
        #[command(flatten)]
        text: TextArgs,
        #[arg(long, value_parser = input::datetime, value_name = "RFC3339")]
        start: DateTime<Utc>,
        #[arg(short, long, value_parser = input::duration, value_name = "30m|PT30M")]
        duration: RelativeDuration,
        #[arg(long, help = "Do not block scheduling")]
        free: bool,
    },
    #[command(about = "Create a date marker")]
    Marker {
        #[command(flatten)]
        text: TextArgs,
        #[arg(long, value_name = "YYYY-MM-DD")]
        date: NaiveDate,
        #[arg(long, value_name = "YYYY-MM-DD", help = "Inclusive final date")]
        end_date: Option<NaiveDate>,
    },
    #[command(about = "Create a signal at a time")]
    Signal {
        #[command(flatten)]
        text: TextArgs,
        #[arg(long, value_parser = input::datetime, value_name = "RFC3339")]
        at: DateTime<Utc>,
    },
    #[command(about = "Create a routine from ordered steps")]
    Routine {
        #[arg(value_parser = input::title)]
        title: String,
        #[arg(long)]
        content: Option<String>,
        #[arg(
            long,
            required = true,
            value_name = "TEXT",
            help = "Repeat for each step, e.g. 'Coffee for 10m'"
        )]
        step: Vec<String>,
        #[arg(long, value_parser = input::schedule_point, value_name = "DATE|RFC3339")]
        target: Option<SchedulePoint>,
    },
}

#[derive(Subcommand)]
pub enum ActionCommand {
    #[command(about = "Complete an action and advance its recurrence")]
    Complete { id: Uuid },
    #[command(about = "Reopen an action without its recurrence")]
    Reopen { id: Uuid },
    #[command(about = "Place an action at the next free slot")]
    Queue { id: Uuid },
    #[command(about = "Remove an action from the schedule")]
    Backlog { id: Uuid },
    #[command(about = "Pin a timed action, or unpin it")]
    Pin {
        id: Uuid,
        #[arg(long)]
        off: bool,
    },
}

#[derive(Subcommand)]
pub enum RoutineCommand {
    #[command(about = "Schedule a routine's steps as actions")]
    Run {
        id: Uuid,
        #[arg(long, value_parser = input::datetime, value_name = "RFC3339")]
        start: Option<DateTime<Utc>>,
    },
}

#[derive(Subcommand)]
pub enum PipelineCommand {
    #[command(about = "Inspect the schedule without changing it")]
    Show {
        #[arg(long, value_enum, default_value = "queue")]
        view: View,
    },
    #[command(about = "Reschedule overdue actions")]
    Refresh,
    #[command(about = "Queue backlogged actions whose dates are due")]
    AutoQueue,
    #[command(about = "Fit queued actions before a horizon")]
    Expedite {
        #[arg(long, value_parser = input::fixed_duration, default_value = "6h", value_name = "DURATION")]
        within: Duration,
    },
    #[command(about = "Find the next free slot without reserving it")]
    NextSlot {
        #[arg(value_parser = input::duration, value_name = "DURATION")]
        duration: RelativeDuration,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum View {
    Queue,
    Backlog,
    Timeline,
}

#[derive(Args)]
pub struct ParseArgs {
    pub kind: ParseKind,
    pub text: String,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ParseKind {
    Action,
    Event,
    Marker,
    Signal,
    ActionTemplate,
    EventTemplate,
    RoutineStep,
}

#[derive(Subcommand)]
pub enum DataCommand {
    #[command(about = "Write the full snapshot to stdout")]
    Export,
    #[command(about = "Merge a snapshot; matching IDs are replaced, nothing is deleted")]
    Import {
        #[arg(value_name = "PATH", help = "JSON file, or - for stdin")]
        file: PathBuf,
    },
}
