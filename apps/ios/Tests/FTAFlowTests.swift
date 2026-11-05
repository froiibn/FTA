import XCTest

// Exercise real native controls and the actual listener; never inject pretend online devices.
final class FTAFlowTests: XCTestCase {
    // Verify the offline Settings route, explicit clipboard action, and Linux readiness message remain reachable in both layouts.
    func testDesktopInstallGuide() {
        for (locale,settings,title,copy,done,confirmation) in [
            ("ko","설정","컴퓨터에 FTA 설치하기","주소 복사","확인","주소를 복사했습니다"),
            ("en","Settings","Install FTA on a computer","Copy address","OK","Address copied")
        ] {
            let app = makeApp(language:locale); app.launch(); storeTab(app,label:settings).tap()
            let entry = app.buttons[title]
            for _ in 0..<8 { if entry.isHittable { break }; app.swipeUp() }
            XCTAssertTrue(entry.isHittable); entry.tap()
            XCTAssertTrue(app.navigationBars[title].waitForExistence(timeout:5))
            capture("desktop-guide-\(locale)-top",app)
            let copyRelease = app.buttons[copy + " https://github.com/froiibn/FTA/releases/tag/v0.1.2"]
            for _ in 0..<5 { if copyRelease.isHittable { break }; app.swipeUp() }
            XCTAssertTrue(copyRelease.isHittable); copyRelease.tap()
            XCTAssertTrue(app.alerts[confirmation].waitForExistence(timeout:3)); app.alerts.buttons[done].tap()
            let linuxStatus = app.staticTexts[locale == "ko" ? "지원 준비 중" : "Support coming soon"]
            for _ in 0..<5 { if linuxStatus.isHittable { break }; app.swipeUp() }
            XCTAssertTrue(linuxStatus.isHittable)
            XCTAssertFalse(app.buttons[copy + " https://github.com/froiibn/FTA#readme"].exists)
            capture("desktop-guide-\(locale)-bottom",app)
            app.navigationBars.buttons.element(boundBy:0).tap()
            XCTAssertTrue(entry.waitForExistence(timeout:3)); app.terminate()
        }
    }
    // Opt-in integration uses a real Mac fixture/Bonjour and the same one-time recipient approval as production.
    func testRealReceiveBanner() throws {
        guard ProcessInfo.processInfo.environment["FTA_IOS_RECEIPT_CHECK"] == "1" else { throw XCTSkip("Run with the real Mac receipt fixture") }
        let app = makeApp(); app.launch()
        let interruption = addUIInterruptionMonitor(withDescription:"FTA notification and LAN permissions") { alert in
            for label in ["허용","Allow"] { if alert.buttons[label].exists { alert.buttons[label].tap(); return true } }
            return false
        }
        defer { removeUIInterruptionMonitor(interruption) }
        app.tabBars.buttons["설정"].tap()
        let trust=app.switches["신뢰하는 집 네트워크입니다"], receive=app.switches["송수신 사용"]
        XCTAssertTrue(trust.waitForExistence(timeout:10))
        let original=receive.value as? String
        if trust.value as? String != "1" { tapSwitch(trust) }
        if receive.value as? String != "1" { tapSwitch(receive) }
        app.tabBars.buttons["보내기"].tap()
        let approval=app.buttons["기기 등록 허용"]
        XCTAssertTrue(approval.waitForExistence(timeout:90)); approval.tap()
        let banner=app.descendants(matching:.any).matching(identifier:"receipt-banner").firstMatch
        XCTAssertTrue(banner.waitForExistence(timeout:15))
        capture("real-receive-banner",app)
        let open=banner.buttons.matching(NSPredicate(format:"label != %@","닫기")).firstMatch
        XCTAssertTrue(open.exists); open.tap()
        XCTAssertTrue(app.buttons["닫기"].waitForExistence(timeout:5))
        capture("real-receive-detail",app)
        app.buttons["닫기"].firstMatch.tap()
        app.tabBars.buttons["설정"].tap()
        if original == "0", receive.value as? String == "1" { tapSwitch(receive) }
    }
    // Capture the real app in each supported language; no invented peers or transfer receipts.
    func testStoreLocalizedScreenshots() throws {
        let path = try XCTUnwrap(Bundle(for:Self.self).url(forResource:"LocaleExpectations",withExtension:"json"))
        let expectations = try JSONDecoder().decode([String:[String:String]].self,from:Data(contentsOf:path))
        let locales = ["ko","en","ja","zh-Hans","zh-Hant","de","fr","es","pt-BR","it","hu"]
        for locale in locales {
            let strings = try XCTUnwrap(expectations[locale == "hu" ? "en" : locale])
            let app = makeApp(language:locale); app.launch()
            XCTAssertTrue(storeTab(app,label:strings["보내기"]!).waitForExistence(timeout:10))
            storeTab(app,label:strings["설정"]!).tap()
            let receiving = app.switches[strings["송수신 사용"]!]
            XCTAssertTrue(receiving.waitForExistence(timeout:5))
            // Pause the simulator's real listener so public captures contain no personal LAN peer names.
            if receiving.value as? String == "1" { tapSwitch(receiving) }
            storeTab(app,label:strings["보내기"]!).tap()
            capture("store-\(locale)-send",app)
            app.buttons[strings["사진"]!].tap()
            capture("store-\(locale)-photos",app)
            // Dismiss the actual photo source popover without selecting or sending any content.
            app.staticTexts["File Transfer Anything"].tap()
            storeTab(app,label:strings["기록"]!).tap()
            capture("store-\(locale)-history",app)
            app.terminate()
        }
    }
    // Foreign labels must remain operable at real accessibility sizes, without changing the layout.
    func testLocalizedLargeTextControls() throws {
        let path = try XCTUnwrap(Bundle(for:Self.self).url(forResource:"LocaleExpectations",withExtension:"json"))
        let expectations = try JSONDecoder().decode([String:[String:String]].self,from:Data(contentsOf:path))
        for locale in ["ko","en","ja","zh-Hans","zh-Hant","de","fr","es","pt-BR","it"] {
            let strings = try XCTUnwrap(expectations[locale])
            let app = makeApp(language:locale); app.launch()
            XCTAssertTrue(storeTab(app,label:strings["보내기"]!).waitForExistence(timeout:10))
            let photos = app.buttons[strings["사진"]!], files = app.buttons[strings["파일"]!]
            for _ in 0..<5 { if photos.isHittable && files.isHittable { break }; app.scrollViews["send-scroll"].swipeUp() }
            XCTAssertTrue(photos.isHittable && files.isHittable,"Media controls must remain reachable in \(locale)")
            capture("accessibility-\(locale)-send",app)
            photos.tap()
            XCTAssertTrue(app.buttons[strings["갤러리에서 선택"]!].waitForExistence(timeout:5))
            XCTAssertTrue(app.buttons[strings["촬영"]!].isHittable)
            capture("accessibility-\(locale)-photos",app)
            app.terminate()
        }
    }
    // Reviewers must be able to read the shipped policy/notices without a server or LAN peer.
    func testBundledPrivacyAndDependencyNotices() {
        let app = makeApp(); app.launch(); app.tabBars.buttons["설정"].tap()
        let policy = app.buttons["개인정보 처리방침"]
        for _ in 0..<8 { if policy.isHittable { break }; app.swipeUp() }
        XCTAssertTrue(policy.isHittable); policy.tap()
        // TextKit exposes selectable paragraphs as separate accessibility elements on current iOS.
        let policyParagraph = app.descendants(matching:.any).matching(NSPredicate(format:"label CONTAINS %@","최초 전송은 수신자가 한 번")).firstMatch
        XCTAssertTrue(policyParagraph.waitForExistence(timeout:5))
        capture("bundled-privacy-policy",app)
        app.navigationBars.buttons.element(boundBy:0).tap()
        let notices = app.buttons["오픈소스 고지"]
        XCTAssertTrue(notices.isHittable); notices.tap()
        // Bundle validation checks the full notice bytes; avoid serializing its entire long accessibility tree.
        XCTAssertTrue(app.navigationBars["오픈소스 고지"].waitForExistence(timeout:5))
        let search = app.searchFields.firstMatch
        XCTAssertTrue(search.waitForExistence(timeout:5)); search.tap(); search.typeText("ring")
        let ring = app.buttons["license-ring-0.17.14"]
        XCTAssertTrue(ring.waitForExistence(timeout:5)); ring.tap()
        XCTAssertTrue(app.navigationBars["ring"].waitForExistence(timeout:5))
        capture("bundled-third-party-notices",app)
    }
    func testNativeTabsAndFilePicker() {
        let app = makeApp(); app.launch()
        XCTAssertTrue(app.tabBars.buttons["보내기"].waitForExistence(timeout:10))
        XCTAssertTrue(app.staticTexts["FTA"].exists && app.staticTexts["File Transfer Anything"].exists)
        XCTAssertFalse(app.navigationBars["보내기"].exists)
        XCTAssertTrue(app.buttons["사진"].exists && app.buttons["파일"].exists)
        XCTAssertTrue(app.buttons["send-message"].exists)
        XCTAssertFalse(app.buttons["send-message"].isEnabled)
        capture("send-glass",app)
        app.tabBars.buttons["기록"].tap()
        XCTAssertTrue(app.navigationBars["기록"].waitForExistence(timeout:5))
        capture("history-glass",app)
        app.tabBars.buttons["설정"].tap()
        XCTAssertTrue(app.switches["신뢰하는 집 네트워크입니다"].waitForExistence(timeout:5))
        XCTAssertTrue(app.textFields["기기 이름"].exists)
        capture("settings-glass",app)
        app.tabBars.buttons["보내기"].tap(); app.buttons["파일"].tap()
        XCTAssertTrue(app.navigationBars["둘러보기"].waitForExistence(timeout:5) || app.buttons["취소"].exists || app.buttons["Cancel"].exists)
        capture("file-picker",app)
    }
    func testBottomComposerRemainsInPlaceWhileContentScrolls() {
        let app = makeApp(); app.launch()
        let input = app.descendants(matching:.any).matching(identifier:"message-input").firstMatch
        XCTAssertTrue(input.waitForExistence(timeout:5))
        let originalY = input.frame.midY
        app.scrollViews["send-scroll"].swipeUp()
        XCTAssertEqual(input.frame.midY,originalY,accuracy:2,"The composer must retain its original bottom position")
        app.scrollViews["send-scroll"].swipeDown()
        capture("restored-bottom-composer-and-glass",app)
    }
    func testTrustedLANAndReceivePreferenceSurviveRestart() throws {
        let app = makeApp(); app.launch(); app.tabBars.buttons["설정"].tap()
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
        let app = makeApp(); app.launch()
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
    // Choose real simulator Photos assets and prove removing one private copy preserves its sibling.
    func testAttachmentPreviewRows() throws {
        let app = makeApp(); app.launch(); app.buttons["사진"].tap(); app.buttons["갤러리에서 선택"].tap()
        let thumbnails = app.images.matching(identifier:"PXGGridLayout-Info")
        XCTAssertTrue(thumbnails.firstMatch.waitForExistence(timeout:10))
        guard thumbnails.count >= 2 else { throw XCTSkip("Seed two real simulator Photos assets first") }
        thumbnails.element(boundBy:0).tap(); thumbnails.element(boundBy:1).tap()
        let confirm = app.buttons.matching(NSPredicate(format:"label BEGINSWITH '추가' OR label BEGINSWITH 'Add' OR label == '완료' OR label == 'Done'")).firstMatch
        confirm.tap(); XCTAssertTrue(app.buttons["send-attachments"].waitForExistence(timeout:15))
        let remove = app.buttons.matching(NSPredicate(format:"label BEGINSWITH %@","선택 해제 "))
        XCTAssertEqual(remove.count,2)
        app.scrollViews["send-scroll"].swipeUp()
        capture("attachment-preview-rows",app)
        remove.firstMatch.tap(); XCTAssertEqual(remove.count,1)
        XCTAssertTrue(app.buttons["send-attachments"].exists)
        capture("attachment-after-single-remove",app)
        app.buttons["선택 해제"].tap(); XCTAssertFalse(app.buttons["send-attachments"].exists)
    }
    func testPhotoLibraryUsesCombinedAttachmentDraft() throws {
        let app = makeApp(); app.launch(); app.buttons["사진"].tap(); app.buttons["갤러리에서 선택"].tap()
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
        let app = makeApp(); app.launch()
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
    // iPad's native floating tab bar exposes cells rather than TabBar descendants on current iPadOS.
    private func storeTab(_ app: XCUIApplication, label: String) -> XCUIElement {
        let phone = app.tabBars.buttons[label]
        if phone.exists { return phone }
        return app.descendants(matching:.any).matching(NSPredicate(format:"label == %@",label)).firstMatch
    }
    // Baseline control tests retain Korean explicitly; global capture checks pass a real app language.
    private func makeApp(language: String = "ko") -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments += ["-AppleLanguages", "(\(language))", "-AppleLocale", language == "ko" ? "ko_KR" : language]
        return app
    }
    private func tapSwitch(_ element: XCUIElement) {
        element.switches.firstMatch.exists ? element.switches.firstMatch.tap() : element.tap()
    }
    private func capture(_ name: String, _ app: XCUIApplication) {
        let attachment = XCTAttachment(screenshot:app.screenshot())
        attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
}
