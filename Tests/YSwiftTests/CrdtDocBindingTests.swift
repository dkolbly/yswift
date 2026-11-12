import XCTest
@testable import Yniffi

private final class Recorder: YrsUpdateObserver, @unchecked Sendable {
    var events: [(update: [UInt8], origin: [UInt8]?)] = []
    func call(update: [UInt8], origin: YrsOrigin?) {
        events.append((update, origin))
    }
}

final class CrdtDocBindingTests: XCTestCase {
    func testClientIdNestedMapsBinaryAndObserver() throws {
        let a = YrsDoc.newWithClientId(clientId: 4242)
        XCTAssertEqual(a.clientId(), 4242)
        let rec = Recorder()
        let sub = try a.observeUpdateV1(delegate: rec)
        _ = sub

        let comps = a.getMap(name: "components")
        let tx = a.transact(origin: Array("local".utf8))
        let inner = comps.insertMap(tx: tx, key: "7")
        inner.insertString(tx: tx, key: "kind", value: "rect")
        inner.insertBinary(tx: tx, key: "pb", value: [0, 1, 255])
        XCTAssertEqual(comps.keyList(tx: tx), ["7"])
        let full = tx.transactionEncodeStateAsUpdate()
        XCTAssertTrue(rec.events.isEmpty)
        tx.free()
        XCTAssertEqual(rec.events.count, 1)
        XCTAssertEqual(rec.events[0].origin, Array("local".utf8))
        XCTAssertEqual(try updateV1ClientIds(update: rec.events[0].update), [4242])

        let b = YrsDoc()
        let recB = Recorder()
        let subB = try b.observeUpdateV1(delegate: recB)
        _ = subB
        let tb = b.transact(origin: Array("remote".utf8))
        try tb.transactionApplyUpdate(update: full)
        tb.free()
        XCTAssertEqual(recB.events.count, 1)
        XCTAssertEqual(recB.events[0].origin, Array("remote".utf8))

        // getMap needs the store, so it must precede an open transaction.
        let bc = b.getMap(name: "components")
        let t2 = b.transact(origin: nil)
        guard case .map? = bc.getValue(tx: t2, key: "7") else { return XCTFail("not a map") }
        let nested = try XCTUnwrap(bc.getMapValue(tx: t2, key: "7"))
        guard case .str(let s)? = nested.getValue(tx: t2, key: "kind") else { return XCTFail("not str") }
        XCTAssertEqual(s, "rect")
        guard case .bin(let d)? = nested.getValue(tx: t2, key: "pb") else { return XCTFail("not bin") }
        XCTAssertEqual(d, [0, 1, 255])
        XCTAssertNil(nested.getValue(tx: t2, key: "nope"))
        XCTAssertTrue(nested.removeKey(tx: t2, key: "kind"))
        XCTAssertEqual(nested.keyList(tx: t2), ["pb"])
        t2.free()
    }
}
