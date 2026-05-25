//! Folder-picker abstraction for app-shell workflows.

use gpui::{Context, PathPromptOptions, Task, Window};
use std::path::PathBuf;

use super::App;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathPickerOutcome {
    Picked(PathBuf),
    Cancelled,
    Failed(String),
}

pub trait PathPicker: 'static {
    fn pick_path(
        &self,
        options: PathPromptOptions,
        window: &mut Window,
        cx: &mut Context<App>,
    ) -> Task<PathPickerOutcome>;
}

#[derive(Default)]
pub struct GpuiPathPicker;

impl PathPicker for GpuiPathPicker {
    fn pick_path(
        &self,
        options: PathPromptOptions,
        window: &mut Window,
        cx: &mut Context<App>,
    ) -> Task<PathPickerOutcome> {
        let receiver = cx.prompt_for_paths(options);

        cx.spawn_in(window, async move |_, _| match receiver.await {
            Ok(Ok(Some(mut paths))) if !paths.is_empty() => {
                PathPickerOutcome::Picked(paths.remove(0))
            }
            Ok(Ok(_)) => PathPickerOutcome::Cancelled,
            Ok(Err(err)) => PathPickerOutcome::Failed(format!("Couldn't open the picker: {err}.")),
            Err(_recv_err) => PathPickerOutcome::Cancelled,
        })
    }
}

pub fn repository_prompt_options() -> PathPromptOptions {
    PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: None,
    }
}

#[cfg(test)]
mod tests {
    use super::repository_prompt_options;

    #[test]
    fn repository_prompt_options_selects_one_directory() {
        let options = repository_prompt_options();

        assert!(!options.files, "repository picker should not allow files");
        assert!(
            options.directories,
            "repository picker should allow directories"
        );
        assert!(
            !options.multiple,
            "repository picker should select one directory"
        );
        assert!(
            options.prompt.is_none(),
            "repository picker has no custom prompt yet"
        );
    }
}
