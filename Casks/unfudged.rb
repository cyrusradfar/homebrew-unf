cask "unfudged" do
  version "0.17.9"
  sha256 "65b7c2759ba596040277b75071e2dd630aa8e723dfb869ca61e26151460b5af9"

  url "https://downloads.unfudged.io/releases/v0.17.9/UNFUDGED-v0.17.9-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
