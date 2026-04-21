cask "unfudged" do
  version "0.18.5"
  sha256 "53272de50ae39d67c232dd154804375611d5f3bb894ac76d635f323979501849"

  url "https://downloads.unfudged.io/releases/v0.18.5/UNFUDGED-v0.18.5-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
