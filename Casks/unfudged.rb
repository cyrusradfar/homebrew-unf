cask "unfudged" do
  version "0.18.2"
  sha256 "0e275e478d6d97326ae551388f2360ed34425f487cca355f6124a585765d6ce6"

  url "https://downloads.unfudged.io/releases/v0.18.2/UNFUDGED-v0.18.2-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
