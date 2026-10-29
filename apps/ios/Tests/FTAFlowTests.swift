import XCTest

// These tests navigate the shipped UI with real empty state; no fake online devices are injected.
final class FTAFlowTests: XCTestCase {
    func testNativeTabsAndFilePicker() {
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.tabBars.buttons["보내기"].waitForExistence(timeout:10))
        XCTAssertTrue(app.staticTexts["송수신 중지됨"].exists)
        // The composer is below the fold on compact screens; verify disabled send after scrolling.
        for _ in 0..<3 where !app.buttons["send-message"].isHittable { app.swipeUp() }
        XCTAssertTrue(app.buttons["send-message"].waitForExistence(timeout:5))
        XCTAssertFalse(app.buttons["send-message"].isEnabled)
        app.swipeDown(); app.swipeDown()
        XCTAssertFalse(app.alerts.firstMatch.exists, "Startup must open protected history without an error")
        capture("send", app)
        app.tabBars.buttons["기록"].tap()
        XCTAssertTrue(app.staticTexts["아직 전송 기록이 없습니다"].waitForExistence(timeout:5))
        capture("history", app)
        app.tabBars.buttons["설정"].tap()
        XCTAssertTrue(app.switches["신뢰하는 집 네트워크입니다"].waitForExistence(timeout:5))
        XCTAssertTrue(app.textFields["기기 이름"].exists)
        capture("settings", app)
        app.tabBars.buttons["보내기"].tap()
        app.buttons["파일 선택"].tap()
        XCTAssertTrue(app.navigationBars["둘러보기"].waitForExistence(timeout:5) || app.buttons["취소"].waitForExistence(timeout:5) || app.buttons["Cancel"].waitForExistence(timeout:5))
        capture("file-picker", app)
    }
    func testForegroundReceiverStopsAfterBackground() throws {
        let app = XCUIApplication(); app.launch()
        app.tabBars.buttons["설정"].tap()
        let trust = app.switches["신뢰하는 집 네트워크입니다"]
        XCTAssertTrue(trust.waitForExistence(timeout:5))
        guard trust.isEnabled else { throw XCTSkip("No usable LAN on the simulator host") }
        addUIInterruptionMonitor(withDescription:"Expected local network consent") { alert in
            for title in ["허용", "Allow"] where alert.buttons[title].exists { alert.buttons[title].tap(); return true }
            return false
        }
        trust.switches.firstMatch.exists ? trust.switches.firstMatch.tap() : trust.tap()
        XCTAssertTrue(app.staticTexts["이 화면이 열려 있는 동안 수신 가능"].waitForExistence(timeout:10))
        XCTAssertFalse(app.alerts.firstMatch.exists, "Polling must accept the notification consumer contract")
        capture("receiver-active", app)
        app.tabBars.buttons["보내기"].tap()
        capture("available-targets", app)
        app.tabBars.buttons["설정"].tap()
        XCUIDevice.shared.press(.home)
        app.activate()
        XCTAssertTrue(app.staticTexts["송수신 중지됨"].waitForExistence(timeout:10))
        // Disconnected/offline registrations must never linger in the send picker after the receiver stops.
        app.tabBars.buttons["보내기"].tap()
        XCTAssertTrue(app.staticTexts["송수신을 시작하세요"].waitForExistence(timeout:5))
        XCTAssertFalse(app.buttons["send-message"].exists && app.buttons["send-message"].isEnabled)
        capture("receiver-paused", app)
    }
    func testPhotoEntryAndUnifiedMessageInput() {
        let app = XCUIApplication(); app.launch()
        XCTAssertTrue(app.buttons["사진 선택"].waitForExistence(timeout:5))
        XCTAssertTrue(app.buttons["사진 촬영"].exists)
        XCTAssertTrue(app.buttons["파일 선택"].exists)
        XCTAssertFalse(app.segmentedControls.buttons["텍스트"].exists)
        XCTAssertFalse(app.segmentedControls.buttons["URL"].exists)
        capture("photo-file-separation", app)
        app.buttons["사진 선택"].tap()
        XCTAssertTrue(app.buttons["취소"].waitForExistence(timeout:5) || app.buttons["Cancel"].waitForExistence(timeout:5))
        let thumbnail = app.images.matching(identifier:"PXGGridLayout-Info").firstMatch
        XCTAssertTrue(thumbnail.waitForExistence(timeout:15))
        capture("photo-library", app)
        thumbnail.tap()
        let confirm = app.buttons.matching(NSPredicate(format:"label BEGINSWITH '추가' OR label BEGINSWITH 'Add' OR label == '완료' OR label == 'Done'")).firstMatch
        XCTAssertTrue(confirm.waitForExistence(timeout:5))
        if confirm.exists { confirm.tap() }
        XCTAssertTrue(app.buttons["사진 보내기"].waitForExistence(timeout:15))
        XCTAssertFalse(app.buttons["파일 보내기"].exists, "Gallery selection must not populate the Files draft")
        XCTAssertFalse(app.alerts.firstMatch.exists)
        capture("photo-prepared", app)
        for _ in 0..<3 where !app.textViews["전송할 텍스트 또는 URL"].isHittable { app.swipeUp() }
        XCTAssertTrue(app.textViews["전송할 텍스트 또는 URL"].exists)
        capture("unified-message", app)
    }
    private func capture(_ name: String, _ app: XCUIApplication) {
        let attachment = XCTAttachment(screenshot:app.screenshot())
        attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
}
