cask "unfudged" do
  version "0.17.3"
  sha256 "d725e5ecfb7bc4ff18fbea7fdf19fc30b649ec62214ba4de40b69c52e6ec398e"

  url "https://downloads.unfudged.io/releases/v0.17.3/UNFUDGED-v0.17.3-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
