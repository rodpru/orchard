# Copy of Casks/orchard.rb in rodpru/homebrew-tap. Update version and sha256 on each release.
cask "orchard" do
  version "0.3.1"
  sha256 "dec3806db75efeeb5d32a28b7502558e0eec52003c0a9371445e2d88dcbb50bf"

  url "https://github.com/rodpru/orchard/releases/download/v#{version}/Orchard_#{version}_universal.dmg"
  name "Orchard"
  desc "Run coding agents in parallel git worktrees"
  homepage "https://rodpru.github.io/orchard/"

  livecheck do
    url :url
    strategy :github_latest
  end

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
