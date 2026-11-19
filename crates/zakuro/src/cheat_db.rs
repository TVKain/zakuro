//! cheats fetched for the running game from the Sharkive database, FlagBrew's
//! collection of Gateway and Action Replay codes, one file a title in the
//! format Citra keeps them in. it is fetched while playing rather than kept
//! in Zakuro, so it is always the database as it is now.

use std::time::Duration;

/// where each title's file is, by its title id.
const SOURCE: &str = "https://raw.githubusercontent.com/FlagBrew/Sharkive/master/3ds";

/// how long the fetch may take before it gives up.
const TIMEOUT: Duration = Duration::from_secs(20);

/// the file of the title's cheats, none when the database has none for it.
pub fn fetch(program_id: u64) -> Result<Option<String>, String> {
    let url = format!("{SOURCE}/{program_id:016X}.txt");
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).http_status_as_error(false).build().into();
    let mut response = agent.get(&url).call().map_err(|error| format!("The cheat database can't be reached: {error}"))?;
    match response.status().as_u16() {
        200 => response.body_mut().read_to_string().map(Some).map_err(|error| format!("The cheat database's answer can't be read: {error}")),
        404 => Ok(None),
        status => Err(format!("The cheat database answered {status}")),
    }
}
