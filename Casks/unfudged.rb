cask "unfudged" do
  version "0.18.4"
  sha256 "4c84b73c42a958a86e24b6b6ab1f014d16164df8510502928c2d498dce8ca159"

  url "https://downloads.unfudged.io/releases/v0.18.4/UNFUDGED-v0.18.4-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
