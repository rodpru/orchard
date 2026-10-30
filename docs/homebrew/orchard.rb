# Copy of Casks/orchard.rb in rodpru/homebrew-tap. Update version and sha256 on each release.
cask "orchard" do
  version "0.3.2"
  sha256 "47e4d0ab71f9b6f039f391d2c2507b7b6a75277220c865292d7776f7246a2387"

  url "https://github.com/rodpru/orchard/releases/download/v#{version}/Orchard_#{version}_universal.dmg"
  name "Orchard"
  desc "Run coding agents in parallel git worktrees"
  homepage "https://rodpru.github.io/orchard/"

  livecheck do
    url :url
    strategy :github_latest
  end

  # Orchard installs its own updates; brew upgrade leaves it alone.
  auto_updates true
  depends_on formula: ["lazygit", "gh"]
  depends_on :macos

  app "Orchard.app"
  binary "#{appdir}/Orchard.app/Contents/MacOS/orchard"

  # The app is not notarized. Remove the quarantine flag so Gatekeeper does not block the first launch.
  postflight_steps do
    run "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "{{appdir}}/Orchard.app"]
  end

  zap trash: [
    "~/Library/Application Support/io.github.rodpru.orchard",
    "~/Library/Caches/io.github.rodpru.orchard",
    "~/Library/WebKit/io.github.rodpru.orchard",
  ]
end
