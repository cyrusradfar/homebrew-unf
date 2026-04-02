cask "unfudged" do
  version "0.18.2"
  sha256 "7240c25256e3c2a05c4cd611150a6d87fc184dab38a3ab1bd0df6b4556d2bf90"

  url "https://downloads.unfudged.io/releases/v0.18.2/UNFUDGED-v0.18.2-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
