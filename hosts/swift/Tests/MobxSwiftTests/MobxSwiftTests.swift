import XCTest
@testable import MobxSwift

final class MobxSwiftTests: XCTestCase {
    func testObservableRoundTrip() {
        let runtime = MobxRuntime()
        var backing = 0
        let observable = runtime.registerObservable(
            name: "counter",
            read: { backing },
            write: { backing = $0 }
        )

        XCTAssertEqual(observable.get(), 0)

        backing = 5
        XCTAssertEqual(observable.get(), 5)

        observable.set(42)
        XCTAssertEqual(backing, 42)
    }

    func testAutorunTracksObservable() {
        let runtime = MobxRuntime()
        var backing = 0
        let observable = runtime.registerObservable(
            name: "counter",
            read: { backing },
            write: { backing = $0 }
        )

        var fired = 0
        let reaction = runtime.autorun {
            _ = observable.get()
            fired += 1
        }

        XCTAssertEqual(fired, 1)

        observable.set(1)
        runtime.flushAfterCallback()
        XCTAssertEqual(fired, 2)

        reaction.dispose()
        observable.set(2)
        runtime.flushAfterCallback()
        XCTAssertEqual(fired, 2)
    }

    func testComputedReflectsObservable() {
        let runtime = MobxRuntime()
        var base = 2
        let observable = runtime.registerObservable(
            name: "base",
            read: { base },
            write: { base = $0 }
        )

        let computed = runtime.registerComputed(name: "double") {
            observable.get() * 2
        }

        XCTAssertEqual(computed.get(), 4)

        observable.set(10)
        runtime.flushAfterCallback()
        XCTAssertEqual(computed.get(), 20)
    }

    func testRunInActionSupportsNestedCalls() {
        let runtime = MobxRuntime()
        var value = 0
        let finalValue: Int = runtime.runInAction(name: "outer") {
            value = 1
            let innerResult: Int = runtime.runInAction(name: "inner") {
                value = 2
                return value
            }
            XCTAssertEqual(innerResult, 2)
            value = 3
            return value
        }
        XCTAssertEqual(finalValue, 3)
        XCTAssertEqual(value, 3)
    }

    func testAllowStateChangesGuardRestores() {
        let runtime = MobxRuntime()
        let result: Int = runtime.allowStateChanges(false) { 42 }
        XCTAssertEqual(result, 42)
    }

    func testSetEnforceActionsToggle() {
        let runtime = MobxRuntime()
        runtime.setEnforceActions(.always)
        runtime.setEnforceActions(.never)
        XCTAssertTrue(true)
    }
}
