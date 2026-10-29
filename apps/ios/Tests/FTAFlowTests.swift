import XCTest

// Exercise real native controls and the actual listener; never inject pretend online devices.
final class FTAFlowTests: XCTestCase {
    func testNativeTabsAndFilePicker() {
        let app = XCUIApplication(); app.launch()
        XCTAssertTrue(app.tabBars.buttons["보내기"].waitForExistence(timeout:10))
        XCTAssertTrue(app.staticTexts["FTA"].exists && app.staticTexts["File Transfer Anything"].exists)
        XCTAssertFalse(app.navigationBars["보내기"].exists)
        XCTAssertTrue(app.buttons["사진"].exists && app.buttons["파일"].exists)
        XCTAssertTrue(app.buttons["send-message"].exists)
        XCTAssertFalse(app.buttons["send-message"].isEnabled)
        capture("send-glass",app)
        app.tabBars.buttons["기록"].tap()
        XCTAssertTrue(app.staticTexts["history-heading"].waitForExistence(timeout:5))
        capture("history-glass",app)
        app.tabBars.buttons["설정"].tap()
        XCTAssertTrue(app.switches["신뢰하는 집 네트워크입니다"].waitForExistence(timeout:5))
        XCTAssertTrue(app.textFields["기기 이름"].exists)
        capture("settings-glass",app)
        app.tabBars.buttons["보내기"].tap(); app.buttons["파일"].tap()
        XCTAssertTrue(app.navigationBars["둘러보기"].waitForExistence(timeout:5) || app.buttons["취소"].exists || app.buttons["Cancel"].exists)
        capture("file-picker",app)
    }
    func testTrustedLANAndReceivePreferenceSurviveRestart() throws {
        let app = XCUIApplication(); app.launch(); app.tabBars.buttons["설정"].tap()
        let trust = app.switches["신뢰하는 집 네트워크입니다"]
        XCTAssertTrue(trust.waitForExistence(timeout:5))
        guard trust.isEnabled else { throw XCTSkip("No usable LAN on simulator host") }
        let receive = app.switches["송수신 사용"]
        if receive.value as? String == "0" { tapSwitch(receive) }
        if trust.value as? String == "0" { tapSwitch(trust) }
        XCTAssertTrue(app.staticTexts["이 화면이 열려 있는 동안 수신 가능"].waitForExistence(timeout:15))
        XCTAssertFalse(app.alerts.firstMatch.exists)
        XCUIDevice.shared.press(.home); app.activate()
        XCTAssertTrue(app.staticTexts["이 화면이 열려 있는 동안 수신 가능"].waitForExistence(timeout:15))
        // A real process termination loses all @Published values; only persisted consent can restore this.
        app.terminate(); app.launch(); app.tabBars.buttons["설정"].tap()
        XCTAssertEqual(trust.value as? String,"1")
        XCTAssertEqual(receive.value as? String,"1")
        XCTAssertTrue(app.staticTexts["이 화면이 열려 있는 동안 수신 가능"].waitForExistence(timeout:15))
        capture("trusted-after-relaunch",app)
        tapSwitch(receive)
        XCTAssertTrue(app.staticTexts["송수신 중지됨"].waitForExistence(timeout:10))
        app.terminate(); app.launch(); app.tabBars.buttons["설정"].tap()
        XCTAssertEqual(trust.value as? String,"1")
        XCTAssertEqual(receive.value as? String,"0")
        XCTAssertTrue(app.staticTexts["송수신 중지됨"].exists)
        capture("preferences-after-relaunch",app)
    }
    func testPhotoSourceChoiceAndGrowingComposer() {
        let app = XCUIApplication(); app.launch()
        app.buttons["사진"].tap()
        XCTAssertTrue(app.buttons["갤러리에서 선택"].waitForExistence(timeout:5))
        XCTAssertTrue(app.buttons["촬영"].exists)
        let photo = app.buttons["사진"]
        let menu = app.buttons["갤러리에서 선택"]
        XCTAssertGreaterThanOrEqual(menu.frame.minY,photo.frame.maxY-4,"The photo menu belongs below the source button")
        capture("photo-source-choice",app)
        // Compact native confirmation popovers can omit the cancel row; dismiss the real picker instead.
        app.buttons["갤러리에서 선택"].tap()
        let cancel = app.buttons["취소"].exists ? app.buttons["취소"] : app.buttons["Cancel"]
        XCTAssertTrue(cancel.waitForExistence(timeout:5)); cancel.tap()
        let input = app.descendants(matching:.any).matching(identifier:"message-input").firstMatch
        XCTAssertTrue(input.exists)
        let initial = input.frame.height
        input.tap(); input.typeText("one\ntwo\nthree\nfour\nfive\nsix")
        let expanded = input.frame.height
        XCTAssertGreaterThan(expanded,initial)
        input.typeText("\nseven\neight")
        XCTAssertEqual(input.frame.height,expanded,accuracy:3,"The field must scroll after reaching four lines")
        XCTAssertFalse(app.buttons["send-message"].isEnabled)
        capture("message-four-line-limit",app)
        input.typeText(String(repeating:XCUIKeyboardKey.delete.rawValue,count:38))
        XCTAssertTrue(app.keyboards.firstMatch.exists)
        // The left padding is genuinely unoccupied; tapping it must dismiss rather than steal field focus.
        app.scrollViews["send-scroll"].coordinate(withNormalizedOffset:CGVector(dx:0.02,dy:0.2)).tap()
        XCTAssertTrue(app.keyboards.firstMatch.waitForNonExistence(timeout:5))
        capture("keyboard-dismissed",app)
    }
    func testPhotoLibraryUsesCombinedAttachmentDraft() throws {
        let app = XCUIApplication(); app.launch(); app.buttons["사진"].tap(); app.buttons["갤러리에서 선택"].tap()
        XCTAssertTrue(app.buttons["취소"].waitForExistence(timeout:5) || app.buttons["Cancel"].exists)
        let thumbnail = app.images.matching(identifier:"PXGGridLayout-Info").firstMatch
        guard thumbnail.waitForExistence(timeout:10) else { throw XCTSkip("No seeded photo in simulator library") }
        thumbnail.tap()
        let confirm = app.buttons.matching(NSPredicate(format:"label BEGINSWITH '추가' OR label BEGINSWITH 'Add' OR label == '완료' OR label == 'Done'")).firstMatch
        XCTAssertTrue(confirm.waitForExistence(timeout:5)); confirm.tap()
        XCTAssertTrue(app.buttons["send-attachments"].waitForExistence(timeout:15))
        XCTAssertFalse(app.alerts.firstMatch.exists)
        capture("combined-photo-draft",app)
        let heading = app.staticTexts["File Transfer Anything"]
        let headingTop = heading.frame.minY
        let photoTop = app.buttons["사진"].frame.minY
        app.scrollViews["send-scroll"].swipeUp()
        let headingDelta = heading.frame.minY-headingTop
        let contentDelta = app.buttons["사진"].frame.minY-photoTop
        // Short pages may fit without scrolling. When content moves, branding must move by the same amount.
        XCTAssertEqual(headingDelta,contentDelta,accuracy:3,"Branding must never become a fixed navigation strip")
        print("Scroll verification: header delta=\(headingDelta), content delta=\(contentDelta)")
        XCTAssertFalse(app.navigationBars["보내기"].exists)
        app.buttons["선택 해제"].tap()
        XCTAssertFalse(app.buttons["send-attachments"].exists)
    }
    // Run against the simulator's actual system appearance; launch defaults do not change UIKit traits.
    func testGlassWithSystemAppearance() {
        let app = XCUIApplication(); app.launch()
        XCTAssertTrue(app.buttons["사진"].waitForExistence(timeout:5))
        capture("glass-system-send",app)
        let heading = app.staticTexts["File Transfer Anything"]
        if heading.frame.height > 25 {
            // The separate real accessibility-size run guarantees content overflow, without fake data.
            let top = heading.frame.minY
            app.scrollViews["send-scroll"].swipeUp()
            XCTAssertLessThan(heading.frame.minY,top-2,"The whole header must scroll at accessibility text sizes")
            XCTAssertFalse(app.navigationBars["보내기"].exists)
            capture("header-scrolled-with-content",app)
        }
        app.tabBars.buttons["설정"].tap(); capture("glass-system-settings",app)
        app.tabBars.buttons["기록"].tap(); capture("glass-system-history",app)
    }
    private func tapSwitch(_ element: XCUIElement) {
        element.switches.firstMatch.exists ? element.switches.firstMatch.tap() : element.tap()
    }
    private func capture(_ name: String, _ app: XCUIApplication) {
        let attachment = XCTAttachment(screenshot:app.screenshot())
        attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
}
