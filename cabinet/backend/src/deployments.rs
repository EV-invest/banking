//! What is deployed in production, and how far behind its source each piece is.
//!
//! Two sources, one page. Flux mounts a ConfigMap into the BFF pod as a directory of
//! `<component>.image` / `<component>.repo` files — that is the ground truth of what
//! runs. GitHub is then asked, per (repository, tag), which commit and pull request the
//! tag points at, and per repository which semver tag is the newest — so the page can say
//! "v0.17.0 is deployed, v0.18.0 is tagged". Nothing here names a component: the
//! directory is listed, never consulted for expected entries.
//!
//! GitHub is optional and rate-limited (60 requests/hour without a token), so every
//! answer is cached in-process: a (repository, tag) never changes and is kept for the
//! process lifetime; the newest tag is re-asked every [`LATEST_TAG_TTL`]; a failure is
//! remembered for [`FAILURE_TTL`] so a broken or exhausted API is not hammered on every
//! page load. A GitHub failure degrades the row (`github_error`), never the page.

use std::{
	collections::{BTreeMap, BTreeSet, HashMap},
	path::{Path, PathBuf},
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

use serde_json::Value;
use tokio::task::JoinSet;
use tracing::Instrument;

/// How long "the newest tag of this repository" is trusted before GitHub is asked again.
const LATEST_TAG_TTL: Duration = Duration::from_secs(10 * 60);
/// How long a GitHub failure (network, rate limit, unknown tag) is remembered before the
/// same question is asked again.
const FAILURE_TTL: Duration = Duration::from_secs(2 * 60);
/// Per-request bound on the GitHub API — the page is interactive, and the router's outer
/// deadline is 15 s for the whole request.
const GITHUB_TIMEOUT: Duration = Duration::from_secs(5);
const GITHUB_API: &str = "https://api.github.com";

/// One `<name>.image` (+ optional `<name>.repo`) pair from the mounted directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployed {
	pub name: String,
	/// The image reference without its tag (`ghcr.io/ev-invest/ev_banking-piggybank`).
	pub image: String,
	pub tag: String,
	/// The `.repo` file as written, trimmed; `None` when the file is absent.
	pub repo_url: Option<String>,
}

/// What GitHub knows about the commit a deployed tag points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
	pub commit_sha: String,
	pub commit_url: String,
	/// RFC 3339, as GitHub reports it.
	pub committed_at: String,
	pub pr: Option<Pull>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pull {
	pub number: u64,
	pub title: String,
	pub url: String,
	pub merged_at: Option<String>,
}

/// One row of the page: the deployed fact, plus whatever GitHub added to it.
#[derive(Debug, Clone)]
pub struct Component {
	pub name: String,
	pub image: String,
	pub tag: String,
	/// `owner/name` when the source lives on GitHub — the key every enrichment hangs off.
	pub repo: Option<String>,
	pub repo_url: Option<String>,
	pub release: Option<Release>,
	pub latest_tag: Option<String>,
	pub github_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Report {
	/// `false` when the directory is absent or holds no `.image` file — local development.
	pub available: bool,
	/// Unix seconds.
	pub fetched_at: i64,
	pub components: Vec<Component>,
}

// ── the mounted directory ──────────────────────────────────────────────────────

/// List the ConfigMap directory. A missing directory is the local-development case and
/// reads as empty; anything else unreadable is logged at `warn` and skipped, because a
/// half-readable directory should still show the half that reads.
pub async fn read_manifest(dir: &Path) -> Vec<Deployed> {
	let mut entries = match tokio::fs::read_dir(dir).await {
		Ok(entries) => entries,
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
		Err(e) => {
			tracing::warn!(dir = %dir.display(), error = %e, "deployed-versions directory is unreadable");
			return Vec::new();
		}
	};

	let mut files = Vec::new();
	loop {
		let entry = match entries.next_entry().await {
			Ok(Some(entry)) => entry,
			Ok(None) => break,
			Err(e) => {
				tracing::warn!(dir = %dir.display(), error = %e, "deployed-versions directory listing failed midway");
				break;
			}
		};
		let name = entry.file_name().to_string_lossy().into_owned();
		// A ConfigMap volume is `..data` → `..<timestamp>/` plus one symlink per key; only
		// the key-named symlinks are ours.
		if name.starts_with("..") || !is_manifest_file(&name) {
			continue;
		}
		match tokio::fs::read_to_string(entry.path()).await {
			Ok(content) => files.push((name, content)),
			Err(e) => tracing::warn!(file = %entry.path().display(), error = %e, "deployed-versions file is unreadable"),
		}
	}
	group_manifest(files)
}

fn is_manifest_file(name: &str) -> bool {
	name.ends_with(".image") || name.ends_with(".repo")
}

/// Pair `<name>.image` with its `<name>.repo`. A `.repo` without an `.image` is nothing
/// deployed and is dropped; an `.image` without a `.repo` is a component GitHub cannot
/// describe, and is kept.
pub fn group_manifest(files: Vec<(String, String)>) -> Vec<Deployed> {
	let mut by_name: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
	for (file, content) in files {
		let content = content.trim();
		if content.is_empty() {
			continue;
		}
		if let Some(name) = file.strip_suffix(".image") {
			by_name.entry(name.to_owned()).or_default().0 = Some(content.to_owned());
		} else if let Some(name) = file.strip_suffix(".repo") {
			by_name.entry(name.to_owned()).or_default().1 = Some(content.to_owned());
		}
	}
	by_name
		.into_iter()
		.filter_map(|(name, (image, repo_url))| {
			let (image, tag) = split_image_ref(&image?);
			Some(Deployed { name, image, tag, repo_url })
		})
		.collect()
}

/// `ghcr.io/ev-invest/x:v0.1.0` → (`ghcr.io/ev-invest/x`, `v0.1.0`). The tag colon is the
/// one after the last `/` — a registry port (`host:5000/x`) sits before it. A digest suffix
/// (`@sha256:…`) is dropped: the page names versions, not blobs. No tag ⇒ `latest`, which
/// is what the runtime pulls.
pub fn split_image_ref(reference: &str) -> (String, String) {
	let reference = reference.split_once('@').map_or(reference, |(before_digest, _)| before_digest);
	let path_start = reference.rfind('/').map_or(0, |i| i + 1);
	match reference[path_start..].split_once(':') {
		Some((name, tag)) if !tag.is_empty() => (format!("{}{name}", &reference[..path_start]), tag.to_owned()),
		_ => (reference.to_owned(), "latest".to_owned()),
	}
}

/// `owner/name` of a GitHub repository URL; `None` for anything that is not one, which
/// simply means no enrichment for that row.
pub fn github_repo(url: &str) -> Option<String> {
	let rest = url.trim().strip_prefix("https://").or_else(|| url.trim().strip_prefix("http://"))?;
	let rest = rest.strip_prefix("www.").unwrap_or(rest);
	let path = rest.strip_prefix("github.com/")?.trim_end_matches('/');
	let path = path.strip_suffix(".git").unwrap_or(path);
	match path.split('/').collect::<Vec<_>>()[..] {
		[owner, name] if !owner.is_empty() && !name.is_empty() => Some(format!("{owner}/{name}")),
		_ => None,
	}
}

// ── semver ─────────────────────────────────────────────────────────────────────

/// `vX.Y.Z` → (X, Y, Z). Anything else — a pre-release (`v1.2.0-rc1`), build metadata,
/// a two-part `v1.2`, no `v` — is `None`: the release tags this org deploys are exactly
/// the three-number form, and everything else is not a release.
pub fn semver(tag: &str) -> Option<(u64, u64, u64)> {
	let mut parts = tag.strip_prefix('v')?.split('.');
	let mut next = || parts.next().filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())).and_then(|p| p.parse().ok());
	let version = (next()?, next()?, next()?);
	parts.next().is_none().then_some(version)
}

/// The highest release tag among `tags`, or `None` when there is no release tag at all.
pub fn newest_semver<'a>(tags: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
	tags.into_iter().filter_map(|tag| semver(tag).map(|v| (v, tag))).max_by_key(|(v, _)| *v).map(|(_, tag)| tag)
}

/// When GitHub links no pull request to the tag's commit (a squash merge from a fork, an
/// old commit), the most recently updated closed PR merged no later than the commit is
/// the best available guess. `pulls` arrives newest-updated first, so the first match
/// wins. GitHub's stamps are all `YYYY-MM-DDTHH:MM:SSZ`, which makes the string order the
/// time order — no parser needed.
pub fn pick_fallback_pull(pulls: Vec<Pull>, committed_at: &str) -> Option<Pull> {
	pulls.into_iter().find(|pull| pull.merged_at.as_deref().is_some_and(|merged| merged <= committed_at))
}

// ── the GitHub side, with its cache ────────────────────────────────────────────

enum Cached<T> {
	Value { value: T, at: Instant },
	Failure { message: String, at: Instant },
}

impl<T: Clone> Cached<T> {
	/// The entry, if still trustworthy: a value outlives `value_ttl` (`None` ⇒ forever),
	/// a failure outlives [`FAILURE_TTL`].
	fn fresh(&self, value_ttl: Option<Duration>) -> Option<Result<T, String>> {
		match self {
			Self::Value { value, at } if value_ttl.is_none_or(|ttl| at.elapsed() < ttl) => Some(Ok(value.clone())),
			Self::Failure { message, at } if at.elapsed() < FAILURE_TTL => Some(Err(message.clone())),
			_ => None,
		}
	}

	fn from_result(result: &Result<T, String>) -> Self {
		let at = Instant::now();
		match result {
			Ok(value) => Self::Value { value: value.clone(), at },
			Err(message) => Self::Failure { message: message.clone(), at },
		}
	}
}

/// The reader + the GitHub client + its cache. One per process, behind [`crate::state::AppState`].
pub struct Deployments {
	dir: PathBuf,
	http: reqwest::Client,
	api_base: String,
	token: Option<String>,
	/// (repository, tag) → release. A tag is immutable once deployed, so a value never expires.
	releases: Mutex<HashMap<(String, String), Cached<Release>>>,
	/// repository → newest release tag (`None` when the repository has no `vX.Y.Z` tag).
	latest: Mutex<HashMap<String, Cached<Option<String>>>>,
	/// Serialises whole refreshes: two admins loading the page at once would otherwise
	/// each spend the same anonymous budget on the same questions.
	refresh: tokio::sync::Mutex<()>,
}

impl Deployments {
	pub fn new(dir: PathBuf, github_token: Option<String>) -> Self {
		Self::with_api_base(dir, github_token, GITHUB_API.to_owned())
	}

	/// `api_base` is the GitHub API origin — a parameter so a test can stand in for it.
	pub fn with_api_base(dir: PathBuf, github_token: Option<String>, api_base: String) -> Self {
		let http = reqwest::Client::builder()
			.timeout(GITHUB_TIMEOUT)
			.user_agent(concat!("cabinet-backend/", env!("CARGO_PKG_VERSION")))
			.build()
			.expect("reqwest client builds with default config");
		Self {
			dir,
			http,
			api_base: api_base.trim_end_matches('/').to_owned(),
			token: github_token,
			releases: Mutex::new(HashMap::new()),
			latest: Mutex::new(HashMap::new()),
			refresh: tokio::sync::Mutex::new(()),
		}
	}

	/// The page: the directory as it is now, enriched from GitHub (cache first, then one
	/// round of requests per repository, repositories in parallel).
	pub async fn report(self: &Arc<Self>) -> Report {
		let fetched_at = crate::util::now_secs();
		let deployed = read_manifest(&self.dir).await;
		let mut components: Vec<Component> = deployed
			.into_iter()
			.map(|d| {
				let repo = d.repo_url.as_deref().and_then(github_repo);
				// A GitHub repository gets a canonical URL so `tag_url` composes cleanly; any
				// other forge is shown as written.
				let repo_url = repo.as_ref().map(|r| format!("https://github.com/{r}")).or(d.repo_url);
				Component {
					name: d.name,
					image: d.image,
					tag: d.tag,
					repo,
					repo_url,
					release: None,
					latest_tag: None,
					github_error: None,
				}
			})
			.collect();
		if components.is_empty() {
			return Report {
				available: false,
				fetched_at,
				components,
			};
		}

		let mut tags_by_repo: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
		for c in &components {
			if let Some(repo) = &c.repo {
				tags_by_repo.entry(repo.clone()).or_default().insert(c.tag.clone());
			}
		}

		let enrichment = {
			let _serialised = self.refresh.lock().await;
			let mut set = JoinSet::new();
			for (repo, tags) in tags_by_repo {
				let me = Arc::clone(self);
				set.spawn(async move { me.enrich_repo(repo, tags).await }.instrument(tracing::Span::current()));
			}
			let mut enrichment: HashMap<String, RepoEnrichment> = HashMap::new();
			while let Some(joined) = set.join_next().await {
				match joined {
					Ok((repo, result)) => {
						enrichment.insert(repo, result);
					}
					Err(e) => tracing::warn!(error = %e, "deployments enrichment task failed"),
				}
			}
			enrichment
		};

		for c in &mut components {
			let Some(found) = c.repo.as_ref().and_then(|repo| enrichment.get(repo)) else { continue };
			let mut errors = Vec::new();
			match found.releases.get(&c.tag) {
				Some(Ok(release)) => c.release = Some(release.clone()),
				Some(Err(e)) => errors.push(e.clone()),
				None => {}
			}
			match &found.latest {
				Ok(latest) => c.latest_tag = latest.clone(),
				Err(e) => errors.push(e.clone()),
			}
			if !errors.is_empty() {
				c.github_error = Some(errors.join("; "));
			}
		}

		// Rows without a repository at the end: they carry the least, and the grouping by
		// repository is what makes four banking images read as one release.
		components.sort_by(|a, b| (a.repo.is_none(), &a.repo, &a.name).cmp(&(b.repo.is_none(), &b.repo, &b.name)));
		Report {
			available: true,
			fetched_at,
			components,
		}
	}

	/// Everything one repository contributes: its newest tag, and each deployed tag's
	/// release. Within a repository the calls run in sequence — there is rarely more than
	/// one tag — while repositories run in parallel from [`Self::report`].
	async fn enrich_repo(&self, repo: String, tags: BTreeSet<String>) -> (String, RepoEnrichment) {
		let latest = self.latest_tag(&repo).await;
		let mut releases = HashMap::new();
		for tag in tags {
			let release = self.release(&repo, &tag).await;
			releases.insert(tag, release);
		}
		(repo, RepoEnrichment { latest, releases })
	}

	async fn release(&self, repo: &str, tag: &str) -> Result<Release, String> {
		let key = (repo.to_owned(), tag.to_owned());
		if let Some(cached) = self.releases.lock().expect("releases cache is never poisoned").get(&key).and_then(|c| c.fresh(None)) {
			return cached;
		}
		let result = self.fetch_release(repo, tag).await;
		if let Err(error) = &result {
			tracing::warn!(%repo, %tag, %error, "GitHub release lookup failed");
		}
		self.releases.lock().expect("releases cache is never poisoned").insert(key, Cached::from_result(&result));
		result
	}

	async fn latest_tag(&self, repo: &str) -> Result<Option<String>, String> {
		if let Some(cached) = self.latest.lock().expect("latest cache is never poisoned").get(repo).and_then(|c| c.fresh(Some(LATEST_TAG_TTL))) {
			return cached;
		}
		let result = self.fetch_latest_tag(repo).await;
		if let Err(error) = &result {
			tracing::warn!(%repo, %error, "GitHub tag listing failed");
		}
		self.latest.lock().expect("latest cache is never poisoned").insert(repo.to_owned(), Cached::from_result(&result));
		result
	}

	async fn fetch_release(&self, repo: &str, tag: &str) -> Result<Release, String> {
		let commit = self.get_json(&format!("/repos/{repo}/commits/{tag}")).await?;
		let commit_sha = str_field(&commit, "/sha").ok_or("commit without sha")?;
		let commit_url = str_field(&commit, "/html_url").ok_or("commit without html_url")?;
		let committed_at = str_field(&commit, "/commit/committer/date").ok_or("commit without committer date")?;

		let linked = self.get_json(&format!("/repos/{repo}/commits/{commit_sha}/pulls")).await?;
		let mut pr = pulls_of(&linked).into_iter().next();
		if pr.is_none() {
			let closed = self.get_json(&format!("/repos/{repo}/pulls?state=closed&sort=updated&direction=desc&per_page=30")).await?;
			pr = pick_fallback_pull(pulls_of(&closed), &committed_at);
		}
		Ok(Release {
			commit_sha,
			commit_url,
			committed_at,
			pr,
		})
	}

	async fn fetch_latest_tag(&self, repo: &str) -> Result<Option<String>, String> {
		let tags = self.get_json(&format!("/repos/{repo}/tags?per_page=100")).await?;
		let names = tags.as_array().ok_or("tags: not a list")?.iter().filter_map(|t| t.get("name").and_then(Value::as_str));
		Ok(newest_semver(names).map(str::to_owned))
	}

	/// One GET against the API. The error string is what the page shows an admin, so it
	/// names the path and the reason and nothing else — never the token.
	async fn get_json(&self, path: &str) -> Result<Value, String> {
		let mut request = self
			.http
			.get(format!("{}{path}", self.api_base))
			.header(reqwest::header::ACCEPT, "application/vnd.github+json")
			.header("X-GitHub-Api-Version", "2022-11-28");
		if let Some(token) = &self.token {
			request = request.bearer_auth(token);
		}
		let response = request.send().await.map_err(|e| format!("{path}: {}", e.without_url()))?;
		let status = response.status();
		if !status.is_success() {
			let remaining = response.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok());
			return Err(if status == reqwest::StatusCode::FORBIDDEN && remaining == Some("0") {
				format!("{path}: GitHub rate limit exhausted")
			} else {
				format!("{path}: HTTP {status}")
			});
		}
		response.json().await.map_err(|e| format!("{path}: {}", e.without_url()))
	}
}

struct RepoEnrichment {
	latest: Result<Option<String>, String>,
	releases: HashMap<String, Result<Release, String>>,
}

fn str_field(v: &Value, pointer: &str) -> Option<String> {
	v.pointer(pointer).and_then(Value::as_str).map(str::to_owned)
}

/// The pull requests in a GitHub list response; entries missing a number are skipped.
fn pulls_of(list: &Value) -> Vec<Pull> {
	list.as_array()
		.map(|pulls| {
			pulls
				.iter()
				.filter_map(|p| {
					Some(Pull {
						number: p.get("number").and_then(Value::as_u64)?,
						title: str_field(p, "/title").unwrap_or_default(),
						url: str_field(p, "/html_url").unwrap_or_default(),
						merged_at: str_field(p, "/merged_at"),
					})
				})
				.collect()
		})
		.unwrap_or_default()
}

#[cfg(test)]
mod tests {
	use std::{
		net::SocketAddr,
		sync::{Arc, Mutex},
	};

	use axum::{Router, extract::State, http::StatusCode, response::IntoResponse};
	use serde_json::json;

	use super::*;

	// ── pure parsing ────────────────────────────────────────────────────────────

	#[test]
	fn an_image_reference_splits_at_the_tag_colon() {
		assert_eq!(
			split_image_ref("ghcr.io/ev-invest/ev_banking-piggybank:v0.17.0"),
			("ghcr.io/ev-invest/ev_banking-piggybank".into(), "v0.17.0".into())
		);
		assert_eq!(split_image_ref("localhost:5000/app:v1.2.3"), ("localhost:5000/app".into(), "v1.2.3".into()));
		assert_eq!(split_image_ref("ghcr.io/ev-invest/app:v1.0.0@sha256:abcd"), ("ghcr.io/ev-invest/app".into(), "v1.0.0".into()));
		assert_eq!(split_image_ref("ghcr.io/ev-invest/app"), ("ghcr.io/ev-invest/app".into(), "latest".into()));
	}

	#[test]
	fn only_github_urls_yield_a_repository() {
		assert_eq!(github_repo("https://github.com/ev-invest/banking"), Some("ev-invest/banking".into()));
		assert_eq!(github_repo("https://github.com/ev-invest/banking.git/"), Some("ev-invest/banking".into()));
		assert_eq!(github_repo("http://www.github.com/EV-invest/lib"), Some("EV-invest/lib".into()));
		assert_eq!(github_repo("https://gitlab.com/ev-invest/banking"), None);
		assert_eq!(github_repo("https://github.com/ev-invest"), None);
		assert_eq!(github_repo("ev-invest/banking"), None);
	}

	#[test]
	fn the_newest_release_tag_is_the_highest_three_number_version() {
		let tags = ["v0.9.0", "v0.10.1", "v0.10.0-rc1", "v1", "v0.2", "0.11.0", "v0.10.1+build", "nightly"];
		assert_eq!(newest_semver(tags), Some("v0.10.1"));
		assert_eq!(newest_semver(["v1.2.0-rc1", "main"]), None);
		assert_eq!(semver("v01.2.3"), Some((1, 2, 3)));
		assert_eq!(semver("v1.2.3.4"), None);
	}

	fn pull(number: u64, merged_at: Option<&str>) -> Pull {
		Pull {
			number,
			title: format!("#{number}"),
			url: format!("https://github.com/o/r/pull/{number}"),
			merged_at: merged_at.map(str::to_owned),
		}
	}

	#[test]
	fn the_fallback_pull_is_the_first_merged_no_later_than_the_commit() {
		let pulls = vec![
			pull(30, Some("2026-09-16T10:00:00Z")),
			pull(29, None),
			pull(28, Some("2026-09-15T09:00:00Z")),
			pull(27, Some("2026-09-14T09:00:00Z")),
		];
		assert_eq!(pick_fallback_pull(pulls.clone(), "2026-09-15T09:00:00Z").map(|p| p.number), Some(28));
		assert_eq!(pick_fallback_pull(pulls, "2026-09-13T00:00:00Z"), None);
	}

	#[test]
	fn the_manifest_pairs_image_with_repo_and_drops_orphans() {
		let deployed = group_manifest(vec![
			("concierge.image".into(), "ghcr.io/ev-invest/concierge:v0.8.0\n".into()),
			("concierge.repo".into(), "https://github.com/ev-invest/concierge\n".into()),
			("tg-sync.image".into(), "ghcr.io/ev-invest/tg-sync:v0.1.0".into()),
			("ghost.repo".into(), "https://github.com/ev-invest/ghost".into()),
			("empty.image".into(), "\n".into()),
		]);
		assert_eq!(
			deployed,
			vec![
				Deployed {
					name: "concierge".into(),
					image: "ghcr.io/ev-invest/concierge".into(),
					tag: "v0.8.0".into(),
					repo_url: Some("https://github.com/ev-invest/concierge".into()),
				},
				Deployed {
					name: "tg-sync".into(),
					image: "ghcr.io/ev-invest/tg-sync".into(),
					tag: "v0.1.0".into(),
					repo_url: None,
				},
			]
		);
	}

	// ── the directory, as a ConfigMap volume lays it out ────────────────────────

	/// A fresh directory shaped like a ConfigMap volume: the keys are symlinks into
	/// `..data`, itself a symlink to a timestamped directory.
	fn configmap_volume(files: &[(&str, &str)]) -> PathBuf {
		// pid + a counter, not a clock: two tests build their volumes within the same
		// microsecond, which is all the resolution the clock has here.
		static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
		let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
		let dir = std::env::temp_dir().join(format!("deployed-versions-{}-{seq}", std::process::id()));
		let data = dir.join("..2026_09_16_10_00_00.000000001");
		std::fs::create_dir_all(&data).unwrap();
		for (name, content) in files {
			std::fs::write(data.join(name), content).unwrap();
			std::os::unix::fs::symlink(Path::new("..data").join(name), dir.join(name)).unwrap();
		}
		std::os::unix::fs::symlink(data.file_name().unwrap(), dir.join("..data")).unwrap();
		dir
	}

	#[tokio::test]
	async fn the_volume_is_read_through_its_symlinks_and_the_dotdot_entries_are_skipped() {
		let dir = configmap_volume(&[
			("ev-banking-piggybank.image", "ghcr.io/ev-invest/ev_banking-piggybank:v0.17.0\n"),
			("ev-banking-piggybank.repo", "https://github.com/ev-invest/banking\n"),
			("notes.txt", "not ours"),
		]);
		let deployed = read_manifest(&dir).await;
		assert_eq!(deployed.len(), 1);
		assert_eq!(deployed[0].name, "ev-banking-piggybank");
		assert_eq!(deployed[0].tag, "v0.17.0");
		assert_eq!(deployed[0].repo_url.as_deref(), Some("https://github.com/ev-invest/banking"));
		std::fs::remove_dir_all(dir).unwrap();
	}

	#[tokio::test]
	async fn a_missing_directory_is_nothing_deployed() {
		let dir = std::env::temp_dir().join(format!("deployed-versions-missing-{}", std::process::id()));
		assert!(read_manifest(&dir).await.is_empty());
		let report = Arc::new(Deployments::new(dir, None)).report().await;
		assert!(!report.available);
		assert!(report.components.is_empty());
	}

	// ── GitHub, stood in for by a local server ──────────────────────────────────

	#[derive(Clone, Default)]
	struct Fake {
		hits: Arc<Mutex<Vec<String>>>,
		/// Answer every request with 403 + an exhausted rate-limit header.
		rate_limited: bool,
	}

	async fn fake_github(State(fake): State<Fake>, uri: axum::http::Uri) -> axum::response::Response {
		let path = uri.path_and_query().map(|p| p.as_str().to_owned()).unwrap_or_default();
		fake.hits.lock().unwrap().push(path.clone());
		if fake.rate_limited {
			return (StatusCode::FORBIDDEN, [("x-ratelimit-remaining", "0")], "rate limited").into_response();
		}
		let body = match path.as_str() {
			"/repos/ev-invest/banking/commits/v0.17.0" => json!({
				"sha": "b16be8e", "html_url": "https://github.com/ev-invest/banking/commit/b16be8e",
				"commit": { "committer": { "date": "2026-09-13T12:00:00Z" } }
			}),
			"/repos/ev-invest/banking/commits/b16be8e/pulls" => json!([
				{ "number": 231, "title": "feat(book): order book", "html_url": "https://github.com/ev-invest/banking/pull/231", "merged_at": "2026-09-13T11:59:00Z" }
			]),
			"/repos/ev-invest/banking/tags?per_page=100" => json!([{ "name": "v0.18.0" }, { "name": "v0.17.0" }, { "name": "v0.18.1-rc1" }]),
			"/repos/ev-invest/concierge/commits/v0.8.0" => json!({
				"sha": "0fb4426", "html_url": "https://github.com/ev-invest/concierge/commit/0fb4426",
				"commit": { "committer": { "date": "2026-09-14T08:00:00Z" } }
			}),
			// No linked PR: the closed-list fallback must pick the one merged before the tag.
			"/repos/ev-invest/concierge/commits/0fb4426/pulls" => json!([]),
			"/repos/ev-invest/concierge/pulls?state=closed&sort=updated&direction=desc&per_page=30" => json!([
				{ "number": 75, "title": "after the tag", "html_url": "https://github.com/ev-invest/concierge/pull/75", "merged_at": "2026-09-15T08:00:00Z" },
				{ "number": 74, "title": "the release", "html_url": "https://github.com/ev-invest/concierge/pull/74", "merged_at": "2026-09-14T07:00:00Z" }
			]),
			"/repos/ev-invest/concierge/tags?per_page=100" => json!([{ "name": "v0.8.0" }]),
			_ => return (StatusCode::NOT_FOUND, "no such fixture").into_response(),
		};
		axum::Json(body).into_response()
	}

	async fn serve(fake: Fake) -> SocketAddr {
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let addr = listener.local_addr().unwrap();
		let app = Router::new().fallback(fake_github).with_state(fake);
		tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
		addr
	}

	fn banking_volume() -> PathBuf {
		configmap_volume(&[
			("ev-banking-signer.image", "ghcr.io/ev-invest/ev_banking-signer:v0.17.0\n"),
			("ev-banking-signer.repo", "https://github.com/ev-invest/banking\n"),
			("ev-banking-piggybank.image", "ghcr.io/ev-invest/ev_banking-piggybank:v0.17.0\n"),
			("ev-banking-piggybank.repo", "https://github.com/ev-invest/banking\n"),
			("concierge.image", "ghcr.io/ev-invest/concierge:v0.8.0\n"),
			("concierge.repo", "https://github.com/ev-invest/concierge\n"),
			("tg-sync.image", "ghcr.io/ev-invest/tg-sync:v0.1.0\n"),
		])
	}

	#[tokio::test]
	async fn the_report_is_enriched_once_per_repository_and_then_served_from_cache() {
		let fake = Fake::default();
		let hits = fake.hits.clone();
		let addr = serve(fake).await;
		let dir = banking_volume();
		let deployments = Arc::new(Deployments::with_api_base(dir.clone(), None, format!("http://{addr}")));

		let report = deployments.report().await;
		assert!(report.available);
		let names: Vec<&str> = report.components.iter().map(|c| c.name.as_str()).collect();
		assert_eq!(
			names,
			["ev-banking-piggybank", "ev-banking-signer", "concierge", "tg-sync"],
			"sorted by (repo, name), repo-less rows last"
		);

		let piggybank = &report.components[0];
		assert_eq!(piggybank.repo.as_deref(), Some("ev-invest/banking"));
		assert_eq!(piggybank.repo_url.as_deref(), Some("https://github.com/ev-invest/banking"));
		let release = piggybank.release.as_ref().expect("the tag's commit is known");
		assert_eq!(release.commit_sha, "b16be8e");
		assert_eq!(release.pr.as_ref().map(|p| p.number), Some(231));
		assert_eq!(piggybank.latest_tag.as_deref(), Some("v0.18.0"), "the rc tag is not a release");
		assert!(piggybank.github_error.is_none());

		let concierge = &report.components[2];
		assert_eq!(
			concierge.release.as_ref().and_then(|r| r.pr.as_ref()).map(|p| p.number),
			Some(74),
			"the fallback skips a PR merged after the tag"
		);
		assert_eq!(concierge.latest_tag.as_deref(), Some("v0.8.0"));

		let tg_sync = &report.components[3];
		assert!(tg_sync.repo.is_none() && tg_sync.release.is_none() && tg_sync.latest_tag.is_none() && tg_sync.github_error.is_none());

		// Two banking images, one tag: one release lookup (2 calls) + one tag listing; the
		// concierge release took 3 calls (fallback) + one tag listing.
		let first_round = hits.lock().unwrap().len();
		assert_eq!(first_round, 7, "requests: {:?}", hits.lock().unwrap());

		let again = deployments.report().await;
		assert_eq!(again.components.len(), 4);
		assert_eq!(hits.lock().unwrap().len(), first_round, "a second page load must be answered from the cache");
		std::fs::remove_dir_all(dir).unwrap();
	}

	#[tokio::test]
	async fn a_github_failure_degrades_the_row_and_is_not_retried_immediately() {
		let fake = Fake {
			rate_limited: true,
			..Fake::default()
		};
		let hits = fake.hits.clone();
		let addr = serve(fake).await;
		let dir = banking_volume();
		let deployments = Arc::new(Deployments::with_api_base(dir.clone(), None, format!("http://{addr}")));

		let report = deployments.report().await;
		assert!(report.available);
		let piggybank = &report.components[0];
		assert!(piggybank.release.is_none() && piggybank.latest_tag.is_none());
		let error = piggybank.github_error.as_deref().expect("the failure is shown");
		assert!(error.contains("rate limit exhausted"), "{error}");
		assert!(
			error.contains("/repos/ev-invest/banking/tags") && error.contains("/repos/ev-invest/banking/commits/v0.17.0"),
			"{error}"
		);

		// One tag listing + one release lookup per repository — the failure short-circuits.
		let first_round = hits.lock().unwrap().len();
		assert_eq!(first_round, 4, "requests: {:?}", hits.lock().unwrap());
		deployments.report().await;
		assert_eq!(hits.lock().unwrap().len(), first_round, "a fresh failure is remembered, not retried on the next load");
		std::fs::remove_dir_all(dir).unwrap();
	}

	/// The real API over real TLS — the one thing the fake cannot vouch for. Off by
	/// default: it needs the network and spends the anonymous budget.
	#[tokio::test]
	#[ignore = "network: hits api.github.com"]
	async fn live_github_answers_over_tls() {
		let deployments = Deployments::new(PathBuf::from("/nonexistent"), std::env::var("GITHUB_TOKEN").ok());
		let latest = deployments.latest_tag("EV-invest/banking").await.expect("the tag listing succeeds");
		assert!(latest.as_deref().is_some_and(|t| semver(t).is_some()), "{latest:?}");
		let release = deployments.release("EV-invest/banking", "v0.17.0").await.expect("the release lookup succeeds");
		assert!(!release.commit_sha.is_empty() && release.committed_at.ends_with('Z'), "{release:?}");
		assert!(release.pr.is_some(), "a tag on a merge commit has a linked pull request: {release:?}");
	}
}
