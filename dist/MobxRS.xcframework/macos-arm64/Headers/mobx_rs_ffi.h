#ifndef MOBX_RS_FFI_H
#define MOBX_RS_FFI_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct { uint64_t raw; } mobx_runtime_t;
typedef struct { uint64_t raw; } mobx_observable_t;
typedef struct { uint64_t raw; } mobx_computed_t;
typedef struct { uint64_t raw; } mobx_reaction_t;
typedef struct { uint64_t raw; } mobx_state_guard_t;

typedef enum {
    MOBX_VALUE_BOOL = 0,
    MOBX_VALUE_I64 = 1,
    MOBX_VALUE_F64 = 2,
    MOBX_VALUE_STRING = 3,
    MOBX_VALUE_JSON = 4,
} MobxValueTag;

typedef struct {
    const uint8_t *ptr;
    uintptr_t len;
} MobxSlice;

typedef union {
    bool bool_value;
    int64_t int_value;
    double float_value;
    MobxSlice string_value;
} MobxValueData;

typedef struct {
    MobxValueTag tag;
    MobxValueData data;
} MobxValue;

typedef MobxValue (*mobx_read_cb)(void *user_data);
typedef void (*mobx_write_cb)(void *user_data, MobxValue value);
typedef void (*mobx_reaction_cb)(void *user_data);

typedef struct {
    const char *name;
    void *user_data;
    mobx_read_cb read;
    mobx_write_cb write;
} mobx_observable_desc;

typedef struct {
    const char *name;
    void *user_data;
    mobx_read_cb getter;
    mobx_write_cb setter;
} mobx_computed_desc;

typedef struct {
    const char *name;
    void *user_data;
    mobx_reaction_cb run;
} mobx_reaction_desc;

typedef struct {
    const char *name;
    void *user_data;
    mobx_reaction_cb body;
} mobx_action_desc;

typedef enum {
    MOBX_ACTION_NEVER = 0,
    MOBX_ACTION_OBSERVED = 1,
    MOBX_ACTION_ALWAYS = 2
} MobxActionPolicy;

mobx_runtime_t mobx_runtime_create(void);
void mobx_runtime_retain(mobx_runtime_t runtime);
void mobx_runtime_release(mobx_runtime_t runtime);
void mobx_runtime_callback_complete(mobx_runtime_t runtime);

mobx_observable_t mobx_observable_register(mobx_runtime_t runtime, const mobx_observable_desc *desc);
MobxValue mobx_observable_get(mobx_observable_t handle);
void mobx_observable_set(mobx_observable_t handle, MobxValue value);
void mobx_observable_report_changed(mobx_observable_t handle);
void mobx_observable_dispose(mobx_observable_t handle);

mobx_computed_t mobx_computed_register(mobx_runtime_t runtime, const mobx_computed_desc *desc);
MobxValue mobx_computed_get(mobx_computed_t handle);
void mobx_computed_set(mobx_computed_t handle, MobxValue value);
void mobx_computed_dispose(mobx_computed_t handle);

mobx_reaction_t mobx_autorun(mobx_runtime_t runtime, const mobx_reaction_desc *desc);
void mobx_reaction_schedule(mobx_reaction_t handle);
void mobx_reaction_dispose(mobx_reaction_t handle);

void mobx_run_in_action(mobx_runtime_t runtime, const mobx_action_desc *desc);
void mobx_set_enforce_actions(MobxActionPolicy policy);
mobx_state_guard_t mobx_allow_state_changes(bool allow);
void mobx_allow_state_changes_end(mobx_state_guard_t guard);

static inline MobxValue MobxValue_bool(bool value) {
    MobxValue v;
    v.tag = MOBX_VALUE_BOOL;
    v.data.bool_value = value;
    return v;
}

static inline MobxValue MobxValue_i64(int64_t value) {
    MobxValue v;
    v.tag = MOBX_VALUE_I64;
    v.data.int_value = value;
    return v;
}

#ifdef __cplusplus
}
#endif

#endif // MOBX_RS_FFI_H
