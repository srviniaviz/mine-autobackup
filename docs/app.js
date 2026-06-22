(async function () {
  const primary = document.getElementById("downloadPrimary");
  const secondary = document.getElementById("downloadSecondary");
  const status = document.getElementById("releaseStatus");
  const versionTable = document.getElementById("versionTable");

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

  function assetKind(asset) {
    const name = asset.name.toLowerCase();
    if (name.endsWith(".exe")) return "Instalador EXE";
    if (name.endsWith(".msi")) return "Instalador MSI";
    return asset.name;
  }

  function releaseDate(value) {
    return new Intl.DateTimeFormat("pt-BR", {
      day: "2-digit",
      month: "short",
      year: "numeric"
    }).format(new Date(value));
  }

  function windowsAssets(release) {
    return (release.assets || []).filter((asset) => {
      const name = asset.name.toLowerCase();
      return name.endsWith(".exe") || name.endsWith(".msi");
    });
  }

  function totalDownloads(assets) {
    return assets.reduce((total, asset) => total + (asset.download_count || 0), 0);
  }

  function renderVersionHistory(releases) {
    const rows = releases
      .map((release, index) => {
        const assets = windowsAssets(release);
        const downloads = assets.length
          ? assets
              .map((asset) => {
                const primaryClass = asset.name.toLowerCase().endsWith(".exe")
                  ? " download-chip--primary"
                  : "";
                return `<a class="download-chip${primaryClass}" href="${asset.browser_download_url}">${assetKind(asset)}</a>`;
              })
              .join("")
          : `<a class="download-chip" href="${release.html_url}">Ver release</a>`;

        return `
          <div class="version-row">
            <span class="version-name">
              ${release.tag_name}
              ${index === 0 ? '<span class="version-current">Atual</span>' : ""}
            </span>
            <span class="version-date">${releaseDate(release.published_at || release.created_at)}</span>
            <span class="version-download-count">${totalDownloads(assets).toLocaleString("pt-BR")}</span>
            <span class="download-links">${downloads}</span>
          </div>
        `;
      })
      .join("");

    versionTable.innerHTML = `
      <div class="version-row version-row--head">
        <span>Versão</span>
        <span>Data</span>
        <span>Total</span>
        <span>Arquivos</span>
      </div>
      ${rows || '<div class="version-empty">Nenhuma versão publicada ainda.</div>'}
    `;
  }

  try {
    const response = await fetch(`https://api.github.com/repos/${repo}/releases`, {
      headers: { Accept: "application/vnd.github+json" }
    });

    if (!response.ok) {
      throw new Error("releases not found");
    }

    const releases = await response.json();
    const release = releases[0];
    if (!release) {
      throw new Error("no releases");
    }

    renderVersionHistory(releases.slice(0, 8));

    const assets = windowsAssets(release);
    const setup =
      assets.find((asset) => asset.name.toLowerCase().endsWith(".exe")) ||
      assets.find((asset) => asset.name.toLowerCase().endsWith(".msi"));
    const msi = assets.find((asset) => asset.name.toLowerCase().endsWith(".msi"));

    if (!setup) {
      throw new Error("release has no Windows installer");
    }

    primary.textContent = `Baixar ${assetKind(setup)}`;
    primary.href = setup.browser_download_url;

    if (msi && msi.browser_download_url !== setup.browser_download_url) {
      secondary.textContent = `Baixar ${assetKind(msi)}`;
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
    versionTable.innerHTML = `
      <div class="version-row version-row--head">
        <span>Versão</span>
        <span>Data</span>
        <span>Total</span>
        <span>Arquivos</span>
      </div>
      <div class="version-empty">Nenhuma release com instalador foi encontrada ainda.</div>
    `;
  }
})();
