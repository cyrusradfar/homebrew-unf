cask "unfudged-staging" do
  version "VERSION_PLACEHOLDER"
  sha256 "SHA256_PLACEHOLDER_UNIVERSAL"

  url "https://downloads.unfudged.io/staging/vVERSION_PLACEHOLDER/UNFUDGED-vVERSION_PLACEHOLDER-universal.dmg"
  name "UNFUDGED (Staging)"
  desc "High-resolution filesystem flight recorder - staging build"
  homepage "https://unfudged.io"
  license "MIT OR Apache-2.0"

  conflicts_with cask: "unfudged"

  depends_on formula: "cyrusradfar/unf/unf-staging"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
