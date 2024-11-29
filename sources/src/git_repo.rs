#[derive(Debug)]
pub struct GitRepo {
    pub url: String,
    pub repo: String,
    pub branch: String,
}

impl GitRepo {
    pub fn from_url(git_url: &str) -> Option<GitRepo> {
        let re = regex::Regex::new(r"^(https?://[^\s@]+@[^\s/]+)/([^\s]+(\.git)?):([^\s]+)$").unwrap();
        if let Some(caps) = re.captures(git_url) {
            let url = caps.get(1)?.as_str().to_string();
            let repo = caps.get(2)?.as_str().to_string();
            let branch = match caps.get(4) {
                Some(branch) => branch.as_str().to_string(),
                None => "main".to_string(),
            };
            Some(GitRepo { url, repo, branch })
        } else {
            None
        }
    }
}