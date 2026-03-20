cask "unfudged" do
  version "0.17.11"
  sha256 "0ba7254654e1d60f91ec957fda8ce4a7c3aa20599fbd61a1388ebc32431f43ae"

  url "https://downloads.unfudged.io/releases/v0.17.11/UNFUDGED-v0.17.11-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
