//! The program a session runs as. `fff roam`, `pick` and `seek`, a window of its own and the file dialogs are one session; what differs
//! between them is said here, by `Program`'s methods, and what only one of
//! them keeps (a dialog's question) lives in its variant of [`Running`], so
//! that it cannot outlast the program. The contents query is the session's,
//! in every program but the dialogs.
use crate::explorer::Dialog;

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
            Self::Roam
            | Self::Window
            | Self::ExplorerOpen
            | Self::ExplorerMultiple
            | Self::ExplorerFolder
            | Self::ExplorerSave => 1,
        }
    }
    /// The current directory can change and folders are listed: all but
    /// `fff seek`, a static choice among the files below its roots.
    pub fn navigates(self) -> bool {
        self != Self::Seek
    }
    /// It answers with paths: chosen files, or the folder it leaves in
    /// (`fff pick`, file dialogs). Roam and seek change the
    /// shell's directory instead, and a window of its own answers nothing.
    pub fn prints_paths(self) -> bool {
        self == Self::Pick || self.dialog()
    }
    /// A file dialog (`fff explorer`), answering the program that asked.
    pub fn dialog(self) -> bool {
        matches!(
            self,
            Self::ExplorerOpen | Self::ExplorerMultiple | Self::ExplorerFolder | Self::ExplorerSave
        )
    }
    /// It starts ready to type: a choice of files, or a save dialog's name.
    pub fn starts_typing(self) -> bool {
        matches!(self, Self::Pick | Self::Seek | Self::ExplorerSave)
    }
    /// How its rows are ranked: `fff pick` and `fff seek` choose files from
    /// deep below, so the best match comes first whatever its depth.
    pub fn rank(self) -> crate::rows::Rank {
        match self {
            Self::Pick | Self::Seek => crate::rows::Rank::Score,
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
}
impl Running {
    /// Runs as `program`, which is not a dialog (`Running::Dialog`).
    pub fn new(program: Program) -> Self {
        match program {
            Program::Roam => Self::Roam,
            Program::Window => Self::Window,
            Program::Pick => Self::Pick,
            Program::Seek => Self::Seek,
            dialog => unreachable!("{dialog:?} runs with its dialog"),
        }
    }
    pub fn program(&self) -> Program {
        match self {
            Self::Roam => Program::Roam,
            Self::Window => Program::Window,
            Self::Pick => Program::Pick,
            Self::Seek => Program::Seek,
            Self::Dialog(dialog) => dialog.program(),
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
