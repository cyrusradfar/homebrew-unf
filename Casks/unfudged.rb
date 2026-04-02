cask "unfudged" do
  version "0.18.3"
  sha256 "a88c789e3262fb86795235b8c524a2ac932d98d14c419f7ee7d0742c381910af"

  url "https://downloads.unfudged.io/releases/v0.18.3/UNFUDGED-v0.18.3-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
