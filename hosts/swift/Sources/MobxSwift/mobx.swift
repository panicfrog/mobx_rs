/// 基础的 store 协议，宏会假设实现类型暴露 `runtime`.
public protocol MobxStore: AnyObject {
    var runtime: MobxRuntime { get }
}

extension MobxStore {
    public func autorun(name: String = "swift::autorun", _ effect: @escaping () -> Void)
        -> MobxReaction
    {
        runtime.autorun(name: name, effect)
    }
}

// Property Wrapper 的便捷扩展
extension MobxStore {
    /// 创建一个 Observable property wrapper
    public func observable<T>(_ initialValue: T, name: String? = nil) -> MobxSwift.Observable<T> {
        MobxSwift.Observable(wrappedValue: initialValue, name: name, runtime: runtime)
    }

    /// 创建一个 Computed property wrapper
    public func computed<T>(_ getter: @escaping () -> T, name: String? = nil)
        -> MobxSwift.Computed<T>
    {
        MobxSwift.Computed(wrappedValue: getter, name: name, runtime: runtime)
    }
}

@propertyWrapper
public final class Observable<T: MobxValueConvertible> {
    private let observable: MobxObservable<T>
    private let storageBox: StorageBox<T>

    public init(wrappedValue: T, name: String? = nil, runtime: MobxRuntime) {
        let propertyName = name ?? "observable"

        // Use a class-based storage box to avoid self-reference issues
        self.storageBox = StorageBox(value: wrappedValue)

        self.observable = runtime.registerObservable(
            name: propertyName,
            read: { [storageBox] in storageBox.value },
            write: { [storageBox] in storageBox.value = $0 }
        )
    }

    public var wrappedValue: T {
        get { observable.get() }
        set { observable.set(newValue) }
    }

    public var projectedValue: MobxObservable<T> {
        observable
    }
}

// Helper class to hold the storage
private final class StorageBox<T> {
    var value: T
    init(value: T) {
        self.value = value
    }
}

@propertyWrapper
public final class Computed<T: MobxValueConvertible> {
    private let computed: MobxComputed<T>

    public init(wrappedValue getValue: @escaping () -> T, name: String? = nil, runtime: MobxRuntime)
    {
        let propertyName = name ?? "computed"
        self.computed = runtime.registerComputed(name: propertyName, getter: getValue)
    }

    public var wrappedValue: T {
        computed.get()
    }

    public var projectedValue: MobxComputed<T> {
        computed
    }
}

@freestanding(expression)
public macro mobxAction<T>(
    runtime: MobxRuntime,
    name: StaticString? = nil,
    _ body: () -> T
) -> T = #externalMacro(module: "MobxSwiftMacros", type: "ActionMacro")
