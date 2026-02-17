cask "unfudged" do
  version "0.14.0"
  sha256 "c5369263a96f9909b95421292b32559fcb245d5cb688ea1a8f76b2c9d649427a"

  url "https://github.com/cyrusradfar/homebrew-unf/releases/download/v0.14.0/UNFUDGED-v0.14.0-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://github.com/cyrusradfar/unfudged"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
