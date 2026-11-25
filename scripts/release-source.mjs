export function releaseRepository(repositoryUrl, override) {
  const configured = /^https:\/\/github\.com\/([^/]+\/[^/]+)\.git$/.exec(
    repositoryUrl,
  )?.[1];
  const source = override ?? configured;
  if (source === "") return null;
  const parts = source?.split("/") ?? [];
  const [owner, repo] = parts;
  if (
    parts.length !== 2 ||
    !/^[a-z\d](?:[a-z\d-]{0,37}[a-z\d])?$/i.test(owner) ||
    !/^[a-z\d._-]{1,100}$/i.test(repo) ||
    [".", ".."].includes(repo) ||
    /\.git$/i.test(repo)
  ) {
    throw new Error("Invalid GitHub release repository. Use OWNER/REPO.");
  }
  return source;
}
