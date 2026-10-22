//! Browser tabs for the windows mode. A Chromium browser (Vivaldi, Chrome) started with
//! `--remote-debugging-port=9222` lists its tabs over the DevTools protocol: `/json/list`
//! gives them, `Browser.getWindowForTarget` (over the browser's websocket) the browser window
//! of each, and `/json/activate/<id>` brings one to the front of its window.
//!
//! dune knows the browser's windows only by title, which is that of their active tab (with
//! the browser's name after it: `Inbox - Vivaldi`): a window owns the tabs of the browser
//! window one of whose tabs it is titled after. No browser on the port: no tabs.

use std::{
    collections::HashMap,
    net::{SocketAddr, TcpStream},
    time::Duration,
};

use serde::Deserialize;

const PORT: u16 = 9222;
/// The browser answers at once or not at all: the finder must not wait for it.
const TIMEOUT: Duration = Duration::from_millis(300);

/// A tab of the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub id: String,
    pub title: String,
    pub url: String,
    /// The browser's window it is in; `None` when that could not be asked.
    pub window: Option<i64>,
}

#[derive(Deserialize)]
struct Target {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
}

#[derive(Deserialize)]
struct Version {
    #[serde(rename = "webSocketDebuggerUrl")]
    websocket: String,
}

fn get(path: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).build().into();
    let mut answer = agent.get(format!("http://127.0.0.1:{PORT}{path}")).call().map_err(|e| e.to_string())?;
    answer.body_mut().read_to_string().map_err(|e| e.to_string())
}

/// The browser's tabs (its pages, not its own devtools), each with its window when known.
pub fn list() -> Vec<Tab> {
    let Some(targets) = get("/json/list").ok().and_then(|json| serde_json::from_str::<Vec<Target>>(&json).ok()) else {
        return Vec::new();
    };
    let mut tabs: Vec<Tab> = targets
        .into_iter()
        .filter(|t| t.kind == "page" && !t.url.starts_with("devtools://"))
        .map(|t| Tab { id: t.id, title: t.title, url: t.url, window: None })
        .collect();
    let ids: Vec<&str> = tabs.iter().map(|t| t.id.as_str()).collect();
    match windows_of(&ids) {
        Ok(windows) => {
            for tab in &mut tabs {
                tab.window = windows.get(&tab.id).copied();
            }
        }
        Err(err) => eprintln!("dune-finder: cannot ask the browser for the windows of its tabs: {err}"),
    }
    tabs
}

/// The browser window of each of the tabs `ids`, asked over one websocket.
fn windows_of(ids: &[&str]) -> Result<HashMap<String, i64>, String> {
    let version: Version = serde_json::from_str(&get("/json/version")?).map_err(|e| e.to_string())?;
    let stream = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], PORT)), TIMEOUT).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    let (mut socket, _) = tungstenite::client(version.websocket.as_str(), stream).map_err(|e| e.to_string())?;
    for (n, id) in ids.iter().enumerate() {
        let ask = serde_json::json!({ "id": n, "method": "Browser.getWindowForTarget", "params": { "targetId": id } });
        socket.send(tungstenite::Message::text(ask.to_string())).map_err(|e| e.to_string())?;
    }
    let mut windows = HashMap::new();
    let mut answers = 0;
    while answers < ids.len() {
        let message = socket.read().map_err(|e| e.to_string())?;
        let Ok(text) = message.to_text() else { continue };
        let Ok(answer) = serde_json::from_str::<serde_json::Value>(text) else { continue };
        let Some(n) = answer["id"].as_u64() else { continue };
        answers += 1;
        if let (Some(id), Some(window)) = (ids.get(n as usize), answer["result"]["windowId"].as_i64()) {
            windows.insert(id.to_string(), window);
        }
    }
    let _ = socket.close(None);
    Ok(windows)
}

/// Bring tab `id` to the front of its browser window.
pub fn activate(id: &str) {
    if let Err(err) = get(&format!("/json/activate/{id}")) {
        eprintln!("dune-finder: cannot activate the tab: {err}");
    }
}

/// A title without the count of unread things in front: `(3) Inbox` → `Inbox`.
pub fn clean(title: &str) -> &str {
    let title = title.trim();
    title
        .strip_prefix('(')
        .and_then(|rest| rest.split_once(')'))
        .filter(|(count, _)| !count.is_empty() && count.chars().all(|c| c.is_ascii_digit()))
        .map_or(title, |(_, rest)| rest.trim_start())
}

/// Whether a window titled `window` shows the tab titled `tab`: the same title, or the
/// browser's name after it (`Inbox - Vivaldi`).
pub fn shows(window: &str, tab: &str) -> bool {
    let (window, tab) = (clean(window), clean(tab));
    !tab.is_empty()
        && window
            .strip_prefix(tab)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(" - ") || rest.starts_with(" — "))
}

/// The tabs of each of the windows `titles` (index → its tabs, in the browser's order): a
/// window has the tabs of the browser window it shows a tab of. Tabs of no window listed
/// (one of a locked desktop, say) are left out.
pub fn by_window(titles: &[&str], tabs: Vec<Tab>) -> HashMap<usize, Vec<Tab>> {
    let mut groups: Vec<(Option<i64>, Vec<Tab>)> = Vec::new();
    for tab in tabs {
        match groups.iter_mut().find(|(w, _)| *w == tab.window) {
            Some((_, group)) => group.push(tab),
            None => groups.push((tab.window, vec![tab])),
        }
    }
    let mut owned = HashMap::new();
    for (_, group) in groups {
        let owner = titles
            .iter()
            .enumerate()
            .find(|(i, title)| !owned.contains_key(i) && group.iter().any(|t| shows(title, &t.title)));
        if let Some((i, _)) = owner {
            owned.insert(i, group);
        }
    }
    owned
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: &str, title: &str, window: i64) -> Tab {
        Tab { id: id.into(), title: title.into(), url: format!("https://{id}.example"), window: Some(window) }
    }

    #[test]
    fn a_window_shows_a_tab_by_its_title() {
        assert!(shows("Inbox - Vivaldi", "(3) Inbox"));
        assert!(shows("(3) Inbox - Vivaldi", "Inbox"));
        assert!(shows("Inbox", "Inbox"));
        assert!(!shows("Inbox zero - Vivaldi", "Inbox"));
        assert!(!shows("Inbox - Vivaldi", ""));
        assert_eq!(clean("(12) Chat"), "Chat");
        assert_eq!(clean("(draft) Chat"), "(draft) Chat");
    }

    #[test]
    fn each_window_gets_the_tabs_of_its_browser_window() {
        let tabs = vec![tab("a", "Mail", 1), tab("b", "Docs", 2), tab("c", "News", 1), tab("d", "Locked", 3)];
        let titles = ["dune — nvim", "Docs - Vivaldi", "News - Vivaldi"];
        let owned = by_window(&titles, tabs);
        let ids = |i| owned[&i].iter().map(|t| t.id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids(2), ["a", "c"]);
        assert_eq!(ids(1), ["b"]);
        // Of no window listed: left out.
        assert_eq!(owned.len(), 2);
    }

    #[test]
    fn two_windows_titled_alike_get_one_browser_window_each() {
        let tabs = vec![tab("a", "New Tab", 1), tab("b", "New Tab", 2)];
        let owned = by_window(&["New Tab - Vivaldi", "New Tab - Vivaldi"], tabs);
        assert_eq!(owned[&0][0].id, "a");
        assert_eq!(owned[&1][0].id, "b");
    }
}
