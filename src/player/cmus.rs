pub fn get_audio_path() -> Option<String> {
    // Try running cmus-remote to get the file path
    // cmus-remote -Q | grep ^file | cut -c 6-

    log::info!("Getting audio path from cmus-remote");
    let cmus_remote_output = match std::process::Command::new("cmus-remote").arg("-Q").output() {
        Ok(output) => {
            let output = String::from_utf8(output.stdout).unwrap();
            output
        }
        Err(e) => {
            log::error!("Error getting cmus-remote output: {:?}", e);
            String::new()
        }
    };

    // Get the file path from the output
    let file_path = cmus_remote_output
        .lines()
        .find(|line| line.starts_with("file "))
        .map(|line| line[5..].to_owned());

    file_path
}
