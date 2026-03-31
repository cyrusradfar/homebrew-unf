cask "unfudged" do
  version "0.18.0"
  sha256 "4518084badda4d6e77a79d03c3593765b2d7f2e62b9d8812c68477a31598d91c"

  url "https://downloads.unfudged.io/releases/v0.18.0/UNFUDGED-v0.18.0-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
