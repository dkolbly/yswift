import XCTest
import Foundation
@testable import Yniffi

final class InteropTests: XCTestCase {
    func perms(_ n: Int) -> [[Int]] {
        if n == 0 { return [[]] }
        var out: [[Int]] = []
        for p in perms(n - 1) { for i in 0...p.count { var q = p; q.insert(n - 1, at: i); out.append(q) } }
        return out
    }
    func svMap(_ b: [UInt8]) -> [UInt64: UInt64] {
        var i = 0
        func v() -> UInt64 { var r: UInt64 = 0; var sh: UInt64 = 0; while true { let x = b[i]; i += 1; r |= UInt64(x & 0x7f) << sh; if x < 0x80 { return r }; sh += 7 } }
        var m: [UInt64: UInt64] = [:]
        let n = v(); for _ in 0..<n { let c = v(); m[c] = v() }
        return m
    }
    func canon(_ o: Any) -> String {
        let d = try! JSONSerialization.data(withJSONObject: o, options: [.sortedKeys, .fragmentsAllowed])
        return String(data: d, encoding: .utf8)!
    }
    func materialize(_ updates: [Data]) throws -> (json: String, sv: [UInt8]) {
        let doc = YrsDoc()
        let txn = doc.transact(origin: nil)
        for u in updates { try txn.transactionApplyUpdate(update: [UInt8](u)) }
        let js = txn.transactionMapsJson(names: ["components", "doc"])
        let sv = txn.transactionStateVector()
        txn.free()
        var obj = try JSONSerialization.jsonObject(with: Data(js.utf8)) as! [String: Any]
        for k in ["components", "doc"] where obj[k] == nil { obj[k] = [String: Any]() }
        return (canon(obj), sv)
    }
    func testVectors() throws {
        let root = URL(fileURLWithPath: ProcessInfo.processInfo.environment["INTEROP_DIR"]!)
        var ran = 0
        for impl in ["go", "yjs", "yrs"] {
            let dir = root.appendingPathComponent(impl)
            let m = try JSONSerialization.jsonObject(with: Data(contentsOf: dir.appendingPathComponent("manifest.json"))) as! [String: Any]
            for s in m["scenarios"] as! [[String: Any]] {
                let name = "\(impl)/\(s["name"]!)"
                let want = canon(s["expected"]!)
                let ups = try (s["updates"] as! [String]).map { try Data(contentsOf: dir.appendingPathComponent($0)) }
                var fails: [String] = []
                var lastSV: [UInt8] = []
                for order in perms(ups.count) {
                    do {
                        let r = try materialize(order.map { ups[$0] })
                        lastSV = r.sv
                        if r.json != want { fails.append("order \(order): got \(r.json.prefix(300)) want \(want.prefix(300))") }
                    } catch { fails.append("order \(order): \(error)") }
                }
                let svFile = [UInt8](try Data(contentsOf: dir.appendingPathComponent(s["sv"] as! String)))
                if svMap(svFile) != svMap(lastSV) { fails.append("sv bytes differ \(svFile) vs \(lastSV)") }
                if let d = s["diff"] as? [String: String] {
                    let base = try Data(contentsOf: dir.appendingPathComponent(d["base"]!))
                    let diff = try Data(contentsOf: dir.appendingPathComponent(d["diff"]!))
                    for o in [[base, diff], [diff, base]] {
                        let r = try materialize(o)
                        if r.json != want { fails.append("base+diff mismatch") }
                    }
                }
                ran += 1
                print(fails.isEmpty ? "PASS \(name)" : "FAIL \(name): \(fails.first!)")
                if !fails.isEmpty { XCTFail("\(name): \(fails.first!)") }
            }
        }
        print("scenarios: \(ran)")
    }
}
