import MobxRSFFI
import Darwin

public typealias CValue = MobxValue

// MARK: - Value Conversion

public protocol MobxValueConvertible {
    static func fromCValue(_ value: CValue) -> Self?
    func toCValue() -> CValue
}

extension Int64: MobxValueConvertible {
    public static func fromCValue(_ value: CValue) -> Int64? {
        value.tag == MOBX_VALUE_I64 ? value.data.int_value : nil
    }

    public func toCValue() -> CValue { MobxValue_i64(self) }
}

extension Int: MobxValueConvertible {
    public static func fromCValue(_ value: CValue) -> Int? {
        Int64.fromCValue(value).map(Int.init)
    }

    public func toCValue() -> CValue {
        Int64(self).toCValue()
    }
}

extension Bool: MobxValueConvertible {
    public static func fromCValue(_ value: CValue) -> Bool? {
        value.tag == MOBX_VALUE_BOOL ? value.data.bool_value : nil
    }

    public func toCValue() -> CValue { MobxValue_bool(self) }
}

extension Double: MobxValueConvertible {
    public static func fromCValue(_ value: CValue) -> Double? {
        value.tag == MOBX_VALUE_F64 ? value.data.float_value : nil
    }

    public func toCValue() -> CValue {
        CValue(tag: MOBX_VALUE_F64, data: MobxValueData(float_value: self))
    }
}

// MARK: - Runtime

public final class MobxRuntime {
    private let handle: mobx_runtime_t

    public init() {
        self.handle = mobx_runtime_create()
    }

    deinit {
        mobx_runtime_release(handle)
    }

    public func flushAfterCallback() {
        mobx_runtime_callback_complete(handle)
    }

    public func registerObservable<T: MobxValueConvertible>(
        name: String = "swift::observable",
        read: @escaping () -> T,
        write: ((T) -> Void)? = nil
    ) -> MobxObservable<T> {
        let box = ObservableBox(runtime: self, read: read, write: write)
        let pointer = box.retainPointer()
        let handle = withCStringCopy(name) { cName -> mobx_observable_t in
            var desc = mobx_observable_desc(
                name: cName,
                user_data: pointer,
                read: observableReadThunk,
                write: observableWriteThunk
            )
            return mobx_observable_register(self.handle, &desc)
        }
        return MobxObservable(handle: handle, storage: box)
    }

    public func registerComputed<T: MobxValueConvertible>(
        name: String = "swift::computed",
        getter: @escaping () -> T,
        setter: ((T) -> Void)? = nil
    ) -> MobxComputed<T> {
        let box = ComputedBox(runtime: self, getter: getter, setter: setter)
        let pointer = box.retainPointer()
        let handle = withCStringCopy(name) { cName -> mobx_computed_t in
            var desc = mobx_computed_desc(
                name: cName,
                user_data: pointer,
                getter: computedGetterThunk,
                setter: computedSetterThunk
            )
            return mobx_computed_register(self.handle, &desc)
        }
        return MobxComputed(handle: handle, storage: box)
    }

    @discardableResult
    public func autorun(name: String = "swift::autorun", _ effect: @escaping () -> Void) -> MobxReaction {
        let box = ReactionBox(runtime: self, effect: effect)
        let pointer = box.retainPointer()
        let handle = withCStringCopy(name) { cName -> mobx_reaction_t in
            var desc = mobx_reaction_desc(name: cName, user_data: pointer, run: reactionThunk)
            return mobx_autorun(self.handle, &desc)
        }
        return MobxReaction(handle: handle, storage: box)
    }

    public func runInAction<T>(name: String = "swift::action", _ body: @escaping () -> T) -> T {
        var result: T?
        let box = ActionBox(runtime: self) {
            result = body()
        }
        let pointer = box.retainPointer()
        withCStringCopy(name) { cName in
            var desc = mobx_action_desc(name: cName, user_data: pointer, body: actionThunk)
            mobx_run_in_action(self.handle, &desc)
        }
        return result!
    }

    // MARK: - Strict mode

    public enum ActionPolicy {
        case never
        case observed
        case always

        fileprivate var ffiValue: MobxActionPolicy {
            switch self {
            case .never: return MOBX_ACTION_NEVER
            case .observed: return MOBX_ACTION_OBSERVED
            case .always: return MOBX_ACTION_ALWAYS
            }
        }
    }

    public func setEnforceActions(_ policy: ActionPolicy) {
        mobx_set_enforce_actions(policy.ffiValue)
    }

    @discardableResult
    public func allowStateChanges<T>(_ allowed: Bool, _ block: () throws -> T) rethrows -> T {
        let guardToken = mobx_allow_state_changes(allowed)
        defer { mobx_allow_state_changes_end(guardToken) }
        return try block()
    }
}

// MARK: - Observable

public final class MobxObservable<T: MobxValueConvertible> {
    private let handle: mobx_observable_t
    private let storage: ObservableBoxBase

    fileprivate init(handle: mobx_observable_t, storage: ObservableBox<T>) {
        self.handle = handle
        self.storage = storage
    }

    deinit {
        mobx_observable_dispose(handle)
        storage.release()
    }

    public func get() -> T {
        guard let value = T.fromCValue(mobx_observable_get(handle)) else {
            fatalError("Failed to convert observable value")
        }
        return value
    }

    public func set(_ newValue: T) {
        mobx_observable_set(handle, newValue.toCValue())
    }
}

// MARK: - Computed

public final class MobxComputed<T: MobxValueConvertible> {
    private let handle: mobx_computed_t
    private let storage: ComputedBoxBase

    fileprivate init(handle: mobx_computed_t, storage: ComputedBox<T>) {
        self.handle = handle
        self.storage = storage
    }

    deinit {
        mobx_computed_dispose(handle)
        storage.release()
    }

    public func get() -> T {
        guard let value = T.fromCValue(mobx_computed_get(handle)) else {
            fatalError("Failed to convert computed value")
        }
        return value
    }

    public func set(_ newValue: T) {
        mobx_computed_set(handle, newValue.toCValue())
    }
}

// MARK: - Reaction

public final class MobxReaction {
    private let handle: mobx_reaction_t
    private let storage: ReactionBox
    private var disposed = false

    fileprivate init(handle: mobx_reaction_t, storage: ReactionBox) {
        self.handle = handle
        self.storage = storage
    }

    deinit {
        dispose()
    }

    public func dispose() {
        guard !disposed else { return }
        mobx_reaction_dispose(handle)
        storage.release()
        disposed = true
    }
}

// MARK: - Boxing infrastructure

private class RetainedBox: AnyObject {
    unowned let runtime: MobxRuntime
    private var retainToken: Unmanaged<RetainedBox>?

    init(runtime: MobxRuntime) {
        self.runtime = runtime
    }

    func retainPointer() -> UnsafeMutableRawPointer {
        let token = Unmanaged.passRetained(self)
        retainToken = token
        return token.toOpaque()
    }

    func release() {
        retainToken?.release()
        retainToken = nil
    }
}

private class ObservableBoxBase: RetainedBox {
    func readValue() -> CValue { fatalError("override") }
    func writeValue(_ value: CValue) { fatalError("override") }
}

private final class ObservableBox<T: MobxValueConvertible>: ObservableBoxBase {
    private let readClosure: () -> T
    private let writeClosure: ((T) -> Void)?

    init(runtime: MobxRuntime, read: @escaping () -> T, write: ((T) -> Void)?) {
        self.readClosure = read
        self.writeClosure = write
        super.init(runtime: runtime)
    }

    override func readValue() -> CValue { readClosure().toCValue() }

    override func writeValue(_ value: CValue) {
        guard let typed = T.fromCValue(value) else { return }
        writeClosure?(typed)
    }
}

private class ComputedBoxBase: RetainedBox {
    func getValue() -> CValue { fatalError("override") }
    func setValue(_ value: CValue) { fatalError("override") }
}

private final class ComputedBox<T: MobxValueConvertible>: ComputedBoxBase {
    private let getter: () -> T
    private let setter: ((T) -> Void)?

    init(runtime: MobxRuntime, getter: @escaping () -> T, setter: ((T) -> Void)?) {
        self.getter = getter
        self.setter = setter
        super.init(runtime: runtime)
    }

    override func getValue() -> CValue { getter().toCValue() }

    override func setValue(_ value: CValue) {
        guard let typed = T.fromCValue(value) else { return }
        setter?(typed)
    }
}

private final class ReactionBox: RetainedBox {
    private let effect: () -> Void

    init(runtime: MobxRuntime, effect: @escaping () -> Void) {
        self.effect = effect
        super.init(runtime: runtime)
    }

    func run() {
        effect()
    }
}

private final class ActionBox: RetainedBox {
    private let body: () -> Void

    init(runtime: MobxRuntime, body: @escaping () -> Void) {
        self.body = body
        super.init(runtime: runtime)
    }

    func run() {
        body()
    }
}

// MARK: - C thunks

private func observableReadThunk(_ userData: UnsafeMutableRawPointer?) -> CValue {
    guard let userData else { return MobxValue_bool(false) }
    let box = Unmanaged<ObservableBoxBase>.fromOpaque(userData).takeUnretainedValue()
    let value = box.readValue()
    box.runtime.flushAfterCallback()
    return value
}

private func observableWriteThunk(_ userData: UnsafeMutableRawPointer?, _ value: CValue) {
    guard let userData else { return }
    let box = Unmanaged<ObservableBoxBase>.fromOpaque(userData).takeUnretainedValue()
    box.writeValue(value)
    box.runtime.flushAfterCallback()
}

private func computedGetterThunk(_ userData: UnsafeMutableRawPointer?) -> CValue {
    guard let userData else { return MobxValue_bool(false) }
    let box = Unmanaged<ComputedBoxBase>.fromOpaque(userData).takeUnretainedValue()
    let value = box.getValue()
    box.runtime.flushAfterCallback()
    return value
}

private func computedSetterThunk(_ userData: UnsafeMutableRawPointer?, _ value: CValue) {
    guard let userData else { return }
    let box = Unmanaged<ComputedBoxBase>.fromOpaque(userData).takeUnretainedValue()
    box.setValue(value)
    box.runtime.flushAfterCallback()
}

private func reactionThunk(_ userData: UnsafeMutableRawPointer?) {
    guard let userData else { return }
    let box = Unmanaged<ReactionBox>.fromOpaque(userData).takeUnretainedValue()
    box.run()
    box.runtime.flushAfterCallback()
}

private func actionThunk(_ userData: UnsafeMutableRawPointer?) {
    guard let userData else { return }
    let box = Unmanaged<ActionBox>.fromOpaque(userData).takeUnretainedValue()
    box.run()
    box.runtime.flushAfterCallback()
    box.release()
}

// MARK: - Helpers

private func withCStringCopy<T>(_ string: String, _ body: (UnsafePointer<CChar>) -> T) -> T {
    guard let duplicated = strdup(string) ?? strdup("swift::mobx") else {
        fatalError("Failed to duplicate string")
    }
    defer { free(UnsafeMutableRawPointer(mutating: duplicated)) }
    return body(UnsafePointer(duplicated))
}
