cask "unfudged" do
  version "0.17.4"
  sha256 "457cf4ac83478ffb5ac85848a6c4e520e207874d1514b52c29bf8ce4f827c79c"

  url "https://downloads.unfudged.io/releases/v0.17.4/UNFUDGED-v0.17.4-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
