cask "unfudged" do
  version "0.17.16"
  sha256 "3de9f394bf1670b24c2152d9d33593a0dae32e3168990c97466c2491807bb35d"

  url "https://downloads.unfudged.io/releases/v0.17.16/UNFUDGED-v0.17.16-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
