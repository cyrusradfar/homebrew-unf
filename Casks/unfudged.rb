cask "unfudged" do
  version "0.17.10"
  sha256 "606df31488f01e7711db3179f13d60197703236d1e1c6d6dca2f0e083be069fc"

  url "https://downloads.unfudged.io/releases/v0.17.10/UNFUDGED-v0.17.10-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
