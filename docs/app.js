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

  function versionedReleases(releases) {
    return releases.filter((release) => /^v?\d+\.\d+\.\d+/i.test(release.tag_name || ""));
  }

  function safeUrl(value, fallback) {
    try {
      const url = new URL(value);
      return url.protocol === "https:" ? url.href : fallback;
    } catch {
      return fallback;
    }
  }

  function appendVersionHeader() {
    const header = document.createElement("div");
    header.className = "version-row version-row--head";
    ["Versão", "Data", "Total", "Arquivos"].forEach((label) => {
      const item = document.createElement("span");
      item.textContent = label;
      header.appendChild(item);
    });
    versionTable.appendChild(header);
  }

  function appendEmptyVersion(message) {
    const empty = document.createElement("div");
    empty.className = "version-empty";
    empty.textContent = message;
    versionTable.appendChild(empty);
  }

  function createDownloadChip(asset, fallbackUrl) {
    const link = document.createElement("a");
    link.className = asset.name.toLowerCase().endsWith(".exe")
      ? "download-chip download-chip--primary"
      : "download-chip";
    link.href = safeUrl(asset.browser_download_url, fallbackUrl);
    link.textContent = assetKind(asset);
    return link;
  }

  function renderVersionHistory(releases) {
    versionTable.replaceChildren();
    appendVersionHeader();

    if (!releases.length) {
      appendEmptyVersion("Nenhuma versão publicada ainda.");
      return;
    }

    releases.forEach((release, index) => {
      const assets = windowsAssets(release);
      const row = document.createElement("div");
      row.className = "version-row";

      const versionName = document.createElement("span");
      versionName.className = "version-name";
      versionName.append(document.createTextNode(release.tag_name || "Sem versão"));
      if (index === 0) {
        const current = document.createElement("span");
        current.className = "version-current";
        current.textContent = "Atual";
        versionName.appendChild(current);
      }

      const date = document.createElement("span");
      date.className = "version-date";
      date.textContent = releaseDate(release.published_at || release.created_at);

      const count = document.createElement("span");
      count.className = "version-download-count";
      count.textContent = totalDownloads(assets).toLocaleString("pt-BR");

      const links = document.createElement("span");
      links.className = "download-links";
      if (assets.length) {
        assets.forEach((asset) => links.appendChild(createDownloadChip(asset, releasesUrl)));
      } else {
        const link = document.createElement("a");
        link.className = "download-chip";
        link.href = safeUrl(release.html_url, releasesUrl);
        link.textContent = "Ver release";
        links.appendChild(link);
      }

      row.append(versionName, date, count, links);
      versionTable.appendChild(row);
    });
  }

  try {
    const response = await fetch(`https://api.github.com/repos/${repo}/releases`, {
      headers: { Accept: "application/vnd.github+json" }
    });

    if (!response.ok) {
      throw new Error("releases not found");
    }

    const releases = versionedReleases(await response.json());
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
    primary.href = safeUrl(setup.browser_download_url, releasesUrl);

    if (msi && msi.browser_download_url !== setup.browser_download_url) {
      secondary.textContent = `Baixar ${assetKind(msi)}`;
      secondary.href = safeUrl(msi.browser_download_url, releasesUrl);
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
    versionTable.replaceChildren();
    appendVersionHeader();
    appendEmptyVersion("Nenhuma release com instalador foi encontrada ainda.");
  }
})();
