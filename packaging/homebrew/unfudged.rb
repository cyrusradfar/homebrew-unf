cask "unfudged" do
  version "VERSION_PLACEHOLDER"
  sha256 "SHA256_PLACEHOLDER_UNIVERSAL"

  url "https://downloads.unfudged.io/releases/vVERSION_PLACEHOLDER/UNFUDGED-vVERSION_PLACEHOLDER-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
