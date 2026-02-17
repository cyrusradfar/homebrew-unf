cask "unfudged" do
  version "0.14.0"
  sha256 "8c927c1d807b9af23cc1f5a6beacfc950965856267b5a6327534c589a7a931ef"

  url "https://github.com/cyrusradfar/homebrew-unf/releases/download/v0.14.0/UNFUDGED-v0.14.0-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://github.com/cyrusradfar/unfudged"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
