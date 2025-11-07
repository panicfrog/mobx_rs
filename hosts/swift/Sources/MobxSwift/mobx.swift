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

/// 创建一个绑定到特定 runtime 的 observable 闭包
/// 使用方式：
/// ```
/// let runtime = MobxRuntime()
/// let observable = makeObservable(runtime: runtime)
/// lazy var count = observable(0, nil)
/// ```
@inlinable
public func makeObservable<T>(runtime: MobxRuntime) -> (T, String?) -> MobxSwift.Observable<T> {
    return { initialValue, name in
        MobxSwift.Observable(wrappedValue: initialValue, name: name, runtime: runtime)
    }
}

/// 创建一个绑定到特定 runtime 的 computed 闭包
/// 使用方式：
/// ```
/// let runtime = MobxRuntime()
/// let computed = makeComputed(runtime: runtime)
/// lazy var doubled = computed({ count * 2 }, nil)
/// ```
@inlinable
public func makeComputed<T>(runtime: MobxRuntime) -> (@escaping () -> T, String?) ->
    MobxSwift.Computed<T>
{
    return { getter, name in
        MobxSwift.Computed(wrappedValue: getter, name: name, runtime: runtime)
    }
}

/// 创建一个绑定到特定 runtime 的 autorun 闭包
/// 使用方式：
/// ```
/// let runtime = MobxRuntime()
/// let autorun = makeAutorun(runtime: runtime)
/// let reaction = autorun("myEffect", { print("effect") })
/// ```
@inlinable
public func makeAutorun(runtime: MobxRuntime) -> (String, @escaping () -> Void) -> MobxReaction {
    return { name, effect in
        runtime.autorun(name: name, effect)
    }
}

// 保留旧的函数名作为别名，以便向后兼容
@inlinable
public func runtimeObservable<T>(runtime: MobxRuntime) -> (T, String?) -> MobxSwift.Observable<T> {
    makeObservable(runtime: runtime)
}

@inlinable
public func runtimeComputed<T>(runtime: MobxRuntime) -> (@escaping () -> T, String?) ->
    MobxSwift.Computed<T>
{
    makeComputed(runtime: runtime)
}

// Property Wrapper 的便捷扩展
extension MobxStore {
    /// 创建一个 Observable property wrapper
    public func observable<T>(_ initialValue: T, name: String? = nil) -> MobxSwift.Observable<T> {
        runtimeObservable(runtime: runtime)(initialValue, name)
    }

    /// 创建一个 Computed property wrapper
    public func computed<T>(_ getter: @escaping () -> T, name: String? = nil)
        -> MobxSwift.Computed<T>
    {
        runtimeComputed(runtime: runtime)(getter, name)
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
