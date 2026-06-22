(async function () {
  const primary = document.getElementById("downloadPrimary");
  const secondary = document.getElementById("downloadSecondary");
  const status = document.getElementById("releaseStatus");

  function inferRepo() {
    const host = window.location.hostname;
    const parts = window.location.pathname.split("/").filter(Boolean);

    if (host.endsWith(".github.io") && parts.length > 0) {
      return `${host.replace(".github.io", "")}/${parts[0]}`;
    }

    return null;
  }

  const repo = inferRepo();
  if (!repo) {
    primary.textContent = "Ver releases";
    primary.href = "https://github.com/";
    secondary.href = "https://github.com/";
    status.textContent = "Configure o GitHub Pages no repositório para ativar downloads automáticos.";
    return;
  }

  const releasesUrl = `https://github.com/${repo}/releases`;
  primary.href = releasesUrl;
  secondary.href = releasesUrl;

  try {
    const response = await fetch(`https://api.github.com/repos/${repo}/releases/latest`, {
      headers: { Accept: "application/vnd.github+json" }
    });

    if (!response.ok) {
      throw new Error("latest release not found");
    }

    const release = await response.json();
    const assets = release.assets || [];
    const setup =
      assets.find((asset) => asset.name.toLowerCase().endsWith(".exe")) ||
      assets.find((asset) => asset.name.toLowerCase().endsWith(".msi"));
    const msi = assets.find((asset) => asset.name.toLowerCase().endsWith(".msi"));

    if (!setup) {
      throw new Error("release has no Windows installer");
    }

    primary.textContent = `Baixar ${setup.name}`;
    primary.href = setup.browser_download_url;

    if (msi && msi.browser_download_url !== setup.browser_download_url) {
      secondary.textContent = `Baixar ${msi.name}`;
      secondary.href = msi.browser_download_url;
    } else {
      secondary.textContent = "Ver releases";
      secondary.href = releasesUrl;
    }

    status.textContent = `Última versão: ${release.tag_name}`;
  } catch {
    primary.textContent = "Ver releases";
    primary.href = releasesUrl;
    secondary.textContent = "Ver builds";
    secondary.href = `https://github.com/${repo}/actions/workflows/build.yml`;
    status.textContent = "Nenhuma release com instalador foi encontrada ainda.";
  }
})();
