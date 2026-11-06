//! The program a session runs as. `fff roam`, `pick` and `seek`, a window of its own, the file dialogs and a list read from stdin are one session; what differs
//! between them is said here, by `Program`'s methods, and what only one of
//! them keeps (a dialog's question, the list's input) lives in its variant
//! of [`Running`], so that it cannot outlast the program. The contents
//! query is the session's, in every program but the dialogs.
use crate::{explorer::Dialog, piped::Piped};

/// What a session runs as: the command fff was started as, or the one
/// `switch_program` went on as. Where keys apply is told by it (a keymap
/// dimension), and so is everything that differs between programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    Roam,
    Window,
    Pick,
    Seek,
    ExplorerOpen,
    ExplorerMultiple,
    ExplorerFolder,
    ExplorerSave,
    /// Bare `fff` with paths on stdin: a choice among them, printed.
    Piped,
}
impl Program {
    /// What `switch_program` offers, by their letters in its chooser: the
    /// programs started from a shell. A file dialog answers its caller and
    /// a window of its own has no shell to leave into, so neither switches.
    pub const SWITCHABLE: [(Self, char); 3] =
        [(Self::Roam, 'r'), (Self::Pick, 'p'), (Self::Seek, 's')];
    /// The depth it searches to without `--depth`: browsing shows a
    /// folder's own entries, picking looks a little deeper, and seeking
    /// deeper still.
    pub fn default_depth(self) -> usize {
        match self {
            Self::Pick => 3,
            Self::Seek => 6,
            // Every listed path is listed, however deep.
            Self::Piped => usize::MAX,
            Self::Roam
            | Self::Window
            | Self::ExplorerOpen
            | Self::ExplorerMultiple
            | Self::ExplorerFolder
            | Self::ExplorerSave => 1,
        }
    }
    /// The current directory can change: all but `fff seek`, a static
    /// choice among the files below its roots, and a list read from stdin.
    pub fn navigates(self) -> bool {
        !matches!(self, Self::Seek | Self::Piped)
    }
    /// Its rows are found by scanning folders, which rescans and live
    /// updates follow: all but a list read from stdin, read once.
    pub fn scans(self) -> bool {
        self != Self::Piped
    }
    /// Only files are listed, whatever `kinds` says: `fff seek`.
    pub fn files_only(self) -> bool {
        self == Self::Seek
    }
    /// It answers with paths: chosen files, or the folder it leaves in
    /// (`fff pick`, file dialogs, a list read from stdin). Roam and seek
    /// change the shell's directory instead, and a window of its own
    /// answers nothing.
    pub fn prints_paths(self) -> bool {
        matches!(self, Self::Pick | Self::Piped) || self.dialog()
    }
    /// A file dialog (`fff explorer`), answering the program that asked.
    pub fn dialog(self) -> bool {
        matches!(
            self,
            Self::ExplorerOpen | Self::ExplorerMultiple | Self::ExplorerFolder | Self::ExplorerSave
        )
    }
    /// A second click on a row is Enter there: in a file dialog, and on a
    /// list read from stdin, where nothing opens. Elsewhere it opens the row
    /// as `l` does.
    pub fn clicks_enter(self) -> bool {
        self.dialog() || self == Self::Piped
    }
    /// It starts ready to type: a choice of files, or a save dialog's name.
    pub fn starts_typing(self) -> bool {
        matches!(
            self,
            Self::Pick | Self::Seek | Self::ExplorerSave | Self::Piped
        )
    }
    /// How its rows are ranked: `fff pick` and `fff seek` choose files from
    /// deep below, so the best match comes first whatever its depth; a list
    /// read from stdin keeps its order among matches as good.
    pub fn rank(self) -> crate::rows::Rank {
        match self {
            Self::Pick | Self::Seek => crate::rows::Rank::Score,
            Self::Piped => crate::rows::Rank::Listed,
            _ => crate::rows::Rank::Depth,
        }
    }
    /// Flat lists start labelled by name, with only the parents that tell
    /// same-named entries apart; elsewhere by their path below the folder
    /// searched (`toggle_labels` switches).
    pub fn short_labels(self) -> bool {
        self == Self::Pick
    }
    /// The program as its command line says it: `roam`, `explorer save`.
    pub fn title(self) -> &'static str {
        match self {
            Self::Roam => "roam",
            Self::Window => "window",
            Self::Pick => "pick",
            Self::Seek => "seek",
            Self::ExplorerOpen => "explorer open",
            Self::ExplorerMultiple => "explorer open-multiple",
            Self::ExplorerFolder => "explorer folder",
            Self::ExplorerSave => "explorer save",
            Self::Piped => "piped",
        }
    }
    /// The session in words, for messages: `fff roam`.
    pub fn described(self) -> String {
        match self {
            Self::Piped => "fff reading paths from stdin".into(),
            program => format!("fff {}", program.title()),
        }
    }
}
/// The program a session runs as, with what only it keeps.
pub enum Running {
    Roam,
    Window,
    Pick,
    Seek,
    /// The file dialog being answered: Enter chooses files in either mode
    /// and q cancels.
    Dialog(Dialog),
    /// The list read from stdin.
    Piped(Box<Piped>),
}
impl Running {
    /// Runs as `program`, which is not a dialog (`Running::Dialog`).
    pub fn new(program: Program) -> Self {
        match program {
            Program::Roam => Self::Roam,
            Program::Window => Self::Window,
            Program::Pick => Self::Pick,
            Program::Seek => Self::Seek,
            other => unreachable!("{other:?} runs with its dialog or list"),
        }
    }
    pub fn program(&self) -> Program {
        match self {
            Self::Roam => Program::Roam,
            Self::Window => Program::Window,
            Self::Pick => Program::Pick,
            Self::Seek => Program::Seek,
            Self::Dialog(dialog) => dialog.program(),
            Self::Piped(_) => Program::Piped,
        }
    }
    pub fn dialog(&self) -> Option<&Dialog> {
        match self {
            Self::Dialog(dialog) => Some(dialog),
            _ => None,
        }
    }
    /// The answer can be several paths, so the marks can be it: `fff pick`
    /// and dialogs choosing several files or folders.
    pub fn returns_marks(&self) -> bool {
        match self {
            Self::Dialog(dialog) => dialog.multiple && dialog.save.is_none(),
            running => running.program().prints_paths(),
        }
    }
}
