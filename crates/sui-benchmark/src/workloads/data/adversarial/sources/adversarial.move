// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

module adversarial::adversarial {
    use std::vector;
    use sui::bcs;
    use sui::object::{Self, UID};
    use sui::tx_context::{Self, TxContext};
    use sui::transfer;
    use sui::event;
    use sui::dynamic_field::{add, borrow};
    use std::string::Self;
    use std::ascii;

    const NUM_DYNAMIC_FIELDS: u64 = 1_000;

    struct S has key, store {
        id: UID,
        contents: vector<u8>
    }

    struct Wrapper has key {
        id: UID,
        s: S,
    }

    // create an object whose Move BCS representation is `n` bytes
    public fun create_object_with_size(n: u64, ctx: &mut TxContext): S {
        // minimum object size for S is 32 bytes for UID + 1 byte for vector length
        assert!(n > std::address::length() + 1, 0);
        let contents = vector[];
        let i = 0;
        let bytes_to_add = n - (std::address::length() + 1);
        while (i < bytes_to_add) {
            vector::push_back(&mut contents, 9);
            i = i + 1;
        };
        let s = S { id: object::new(ctx), contents };
        let size = vector::length(&bcs::to_bytes(&s));
        // shrink by 1 byte until we match size. mismatch happens because of len(UID) + vector length byte
        while (size > n) {
            let _ = vector::pop_back(&mut s.contents);
            // hack: assume this doesn't change the size of the BCS length byte
            size = size - 1;
        };
        // double-check that we got the size right
        assert!(vector::length(&bcs::to_bytes(&s)) == n, 1);
        s
    }

    /// Create `n` owned objects of size `size` and transfer them to the tx sender
    public fun create_owned_objects(n: u64, size: u64, ctx: &mut TxContext) {
        let i = 0;
        let sender = tx_context::sender(ctx);
        while (i < n) {
            transfer::public_transfer(create_object_with_size(size, ctx), sender);
            i = i + 1
        }
    }

    struct NewValueEvent has copy, drop {
        contents: vector<u8>
    }

    // TODO: factor out the common bits with `create_object_with_size`
    // emit an event of size n
    public fun emit_event_with_size(n: u64) {
        // 55 seems to be the added size from event size derivation for `NewValueEvent`
        assert!(n > 55, 0);
        n = n - 55;
        // minimum object size for NewValueEvent is 1 byte for vector length
        assert!(n > 1, 0);
        let contents = vector[];
        let i = 0;
        let bytes_to_add = n - 1;
        while (i < bytes_to_add) {
            vector::push_back(&mut contents, 9);
            i = i + 1;
        };
        let s = NewValueEvent { contents };
        let size = vector::length(&bcs::to_bytes(&s));
        // shrink by 1 byte until we match size. mismatch happens because of len(UID) + vector length byte
        while (size > n) {
            let _ = vector::pop_back(&mut s.contents);
            // hack: assume this doesn't change the size of the BCS length byte
            size = size - 1;
        };

        event::emit(s);
    }

    // create `n` vectors of size `size` bytes
    // vectors are transient. Idea here is to exhaust memory
    public fun create_vectors_with_size(n: u64, size: u64) {
        let top_level = vector[];
        while (n > 0) {
            let contents = vector[];
            let i = size;
            while (i > 0) {
                vector::push_back(&mut contents, 0u256);
                i = i - 1;
            };
            vector::push_back(&mut top_level, contents);
            n = n - 1;
        };
    }    

    struct Obj has key, store {
        id: object::UID,
    }

    public fun add_dynamic_fields(obj: &mut Obj, n: u64) {
        let i = 0;
        while (i < n) {
            add<u64, u64>(&mut obj.id, i, i);
            i = i + 1;
        };
    }

    public fun read_n_dynamic_fields(obj: &mut Obj, n: u64) {
        let i = 0;
        while (i < n) {
            let _ = borrow<u64, u64>(&obj.id, i);
            i = i + 1;
        };
    }

    /// Emit `n` events of size `size`
    public fun emit_events(n: u64, size: u64) {
        let i = 0;
        while (i < n) {
            emit_event_with_size(size);
            i = i + 1
        }
    }

    /// Create `n` objects of size `size` and share them
    public fun create_shared_objects(n: u64, size: u64, ctx: &mut TxContext) {
        let i = 0;
        while (i < n) {
            transfer::public_share_object(create_object_with_size(size, ctx));
            i = i + 1
        }
    }

    // function which takes a lot of params
    public fun lots_of_params(arg_0: vector<u8>, arg_1: vector<u8>, arg_2: vector<u8>, arg_3: vector<u8>, arg_4: vector<u8>, arg_5: vector<u8>, arg_6: vector<u8>, arg_7: vector<u8>, arg_8: vector<u8>, arg_9: vector<u8>, arg_10: vector<u8>, arg_11: vector<u8>, arg_12: vector<u8>, arg_13: vector<u8>, arg_14: vector<u8>, arg_15: vector<u8>, arg_16: vector<u8>, arg_17: vector<u8>, arg_18: vector<u8>, arg_19: vector<u8>, arg_20: vector<u8>, arg_21: vector<u8>, arg_22: vector<u8>, arg_23: vector<u8>, arg_24: vector<u8>, arg_25: vector<u8>, arg_26: vector<u8>, arg_27: vector<u8>, arg_28: vector<u8>, arg_29: vector<u8>, arg_30: vector<u8>, arg_31: vector<u8>, arg_32: vector<u8>, arg_33: vector<u8>, arg_34: vector<u8>, arg_35: vector<u8>, arg_36: vector<u8>, arg_37: vector<u8>, arg_38: vector<u8>, arg_39: vector<u8>, arg_40: vector<u8>, arg_41: vector<u8>, arg_42: vector<u8>, arg_43: vector<u8>, arg_44: vector<u8>, arg_45: vector<u8>, arg_46: vector<u8>, arg_47: vector<u8>, arg_48: vector<u8>, arg_49: vector<u8>, arg_50: vector<u8>, arg_51: vector<u8>, arg_52: vector<u8>, arg_53: vector<u8>, arg_54: vector<u8>, arg_55: vector<u8>, arg_56: vector<u8>, arg_57: vector<u8>, arg_58: vector<u8>, arg_59: vector<u8>, arg_60: vector<u8>, arg_61: vector<u8>, arg_62: vector<u8>, arg_63: vector<u8>, arg_64: vector<u8>, arg_65: vector<u8>, arg_66: vector<u8>, arg_67: vector<u8>, arg_68: vector<u8>, arg_69: vector<u8>, arg_70: vector<u8>, arg_71: vector<u8>, arg_72: vector<u8>, arg_73: vector<u8>, arg_74: vector<u8>, arg_75: vector<u8>, arg_76: vector<u8>, arg_77: vector<u8>, arg_78: vector<u8>, arg_79: vector<u8>, arg_80: vector<u8>, arg_81: vector<u8>, arg_82: vector<u8>, arg_83: vector<u8>, arg_84: vector<u8>, arg_85: vector<u8>, arg_86: vector<u8>, arg_87: vector<u8>, arg_88: vector<u8>, arg_89: vector<u8>, arg_90: vector<u8>, arg_91: vector<u8>, arg_92: vector<u8>, arg_93: vector<u8>, arg_94: vector<u8>, arg_95: vector<u8>, arg_96: vector<u8>, arg_97: vector<u8>, arg_98: vector<u8>, arg_99: vector<u8>, arg_100: vector<u8>, arg_101: vector<u8>, arg_102: vector<u8>, arg_103: vector<u8>, arg_104: vector<u8>, arg_105: vector<u8>, arg_106: vector<u8>, arg_107: vector<u8>, arg_108: vector<u8>, arg_109: vector<u8>, arg_110: vector<u8>, arg_111: vector<u8>, arg_112: vector<u8>, arg_113: vector<u8>, arg_114: vector<u8>, arg_115: vector<u8>, arg_116: vector<u8>, arg_117: vector<u8>, arg_118: vector<u8>, arg_119: vector<u8>, arg_120: vector<u8>, arg_121: vector<u8>, arg_122: vector<u8>, arg_123: vector<u8>, arg_124: vector<u8>, arg_125: vector<u8>, arg_126: vector<u8>, arg_127: vector<u8>) {
        let contents = vector[];
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_0)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_1)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_2)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_3)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_4)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_5)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_6)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_7)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_8)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_9)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_10)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_11)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_12)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_13)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_14)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_15)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_16)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_17)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_18)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_19)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_20)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_21)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_22)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_23)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_24)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_25)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_26)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_27)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_28)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_29)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_30)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_31)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_32)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_33)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_34)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_35)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_36)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_37)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_38)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_39)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_40)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_41)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_42)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_43)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_44)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_45)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_46)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_47)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_48)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_49)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_50)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_51)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_52)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_53)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_54)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_55)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_56)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_57)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_58)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_59)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_60)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_61)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_62)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_63)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_64)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_65)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_66)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_67)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_68)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_69)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_70)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_71)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_72)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_73)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_74)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_75)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_76)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_77)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_78)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_79)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_80)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_81)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_82)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_83)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_84)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_85)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_86)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_87)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_88)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_89)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_90)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_91)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_92)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_93)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_94)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_95)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_96)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_97)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_98)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_99)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_100)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_101)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_102)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_103)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_104)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_105)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_106)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_107)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_108)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_109)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_110)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_111)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_112)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_113)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_114)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_115)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_116)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_117)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_118)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_119)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_120)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_121)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_122)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_123)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_124)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_125)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_126)));
        vector::push_back(&mut contents, string::from_ascii(ascii::string(arg_127)));
    }   


    /// Initialize object to be used for dynamic field opers
    fun init(ctx: &mut TxContext) {
        let id = object::new(ctx);
        let x = Obj { id };
        add_dynamic_fields(&mut x, NUM_DYNAMIC_FIELDS);
        transfer::share_object(x);
    }
}
