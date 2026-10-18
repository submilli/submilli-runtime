------------------------- MODULE QuotaAccounting -------------------------
EXTENDS Naturals, Integers, FiniteSets, TLC

CONSTANTS Operation, Limit, MaxCrashes, Fault, Witness, ReplacementBytes
ASSUME /\ Operation \in {"replace", "append", "remove", "move"}
       /\ Limit \in Nat /\ MaxCrashes \in Nat /\ ReplacementBytes \in 1..4
       /\ Fault \in {"none", "count-private", "repeat-delta", "skip-rebuild",
                      "skip-admission"}

Paths == {"source", "destination"}
Files == {"old", "destination", "new"}
InitialNames == [p \in Paths |-> IF p = "source" THEN "old" ELSE "destination"]
InitialSizes == [f \in Files |-> IF f = "new" THEN ReplacementBytes ELSE 2]
RECURSIVE Sum(_, _)
Sum(ids, sizes) == IF ids = {} THEN 0 ELSE
    LET id == CHOOSE f \in ids: TRUE IN sizes[id] + Sum(ids \ {id}, sizes)
Usage(names, sizes) == Sum({names[p] : p \in Paths} \ {"absent"}, sizes)

VARIABLES names, sizes, diskNames, diskSizes, used, privateBytes,
          phase, receipt, up, crashes, before, after, seen
vars == <<names, sizes, diskNames, diskSizes, used, privateBytes,
          phase, receipt, up, crashes, before, after, seen>>

Init ==
    /\ names = InitialNames /\ sizes = InitialSizes
    /\ diskNames = names /\ diskSizes = sizes
    /\ used = Usage(names, sizes) /\ privateBytes = 0
    /\ phase = "initial" /\ receipt = "pending" /\ up = TRUE
    /\ crashes = 0 /\ before = 0 /\ after = 0 /\ seen = {}

Stage ==
    /\ up /\ receipt = "pending" /\ phase = "initial"
    /\ privateBytes' = 10
    /\ used' = used + IF Fault = "count-private" THEN 10 ELSE 0
    /\ phase' = "publish"
    /\ UNCHANGED <<names, sizes, diskNames, diskSizes, receipt, up, crashes,
                   before, after, seen>>

(* Repeated replacement attempts publish again even when Finished. "new"
   abstracts fresh inodes of the same size: only one is public at a time.
   These names/sizes describe ONE native mutation. Append exposes one byte per
   write; moving adds a second name for the same inode before removing source. *)
NextNames ==
    CASE Operation = "replace" -> [names EXCEPT !["source"] = "new"]
      [] Operation = "remove" -> [names EXCEPT !["source"] = "absent"]
      [] Operation = "move" ->
          IF names["destination"] # "old"
          THEN [names EXCEPT !["destination"] = "old"]
          ELSE [names EXCEPT !["source"] = "absent"]
      [] OTHER -> names
NextSizes == IF Operation = "append" /\ sizes["old"] < 4
             THEN [sizes EXCEPT !["old"] = @ + 1] ELSE sizes
FinalSize == CASE Operation = "replace" -> ReplacementBytes + 2
              [] Operation = "append" -> 6
              [] Operation = "remove" -> 2
              [] OTHER -> 2
Finished == CASE Operation = "replace" -> names["source"] = "new"
             [] Operation = "append" -> sizes["old"] = 4
             [] Operation = "remove" -> names["source"] = "absent"
             [] OTHER -> names["source"] = "absent"
Fits == FinalSize <= used \/ FinalSize <= Limit

Refuse ==
    /\ up /\ receipt = "pending" /\ phase = "publish" /\ ~Fits
    /\ Fault # "skip-admission"
    /\ receipt' = "refused" /\ phase' = "done"
    /\ UNCHANGED <<names, sizes, diskNames, diskSizes, used, privateBytes,
                   up, crashes, before, after, seen>>

Publish ==
    /\ up /\ receipt = "pending" /\ phase = "publish"
    /\ (~Finished \/ Operation = "replace")
    /\ (Fits \/ Fault = "skip-admission")
    /\ before' = Usage(names, sizes) /\ after' = Usage(NextNames, NextSizes)
    /\ names' = NextNames /\ sizes' = NextSizes
    /\ phase' = "account"
    /\ UNCHANGED <<diskNames, diskSizes, used, privateBytes, receipt,
                   up, crashes, seen>>

Account ==
    /\ up /\ phase = "account"
    /\ used' = used + after - before
    /\ phase' = IF Operation = "replace" THEN "sync-replacement" ELSE "publish"
    /\ UNCHANGED <<names, sizes, diskNames, diskSizes, privateBytes, receipt,
                   up, crashes, before, after, seen>>

Sync ==
    /\ up /\ receipt = "pending" /\ Finished
    /\ (phase = "sync-replacement" \/ (phase = "publish" /\ Operation # "replace"))
    /\ diskNames' = names /\ diskSizes' = sizes /\ phase' = "receipt"
    /\ UNCHANGED <<names, sizes, used, privateBytes, receipt,
                   up, crashes, before, after, seen>>

SaveReceipt ==
    /\ up /\ phase = "receipt" /\ receipt = "pending"
    /\ receipt' = "done" /\ phase' = "done"
    /\ UNCHANGED <<names, sizes, diskNames, diskSizes, used, privateBytes,
                   up, crashes, before, after, seen>>

Retry ==
    /\ up /\ receipt = "done" /\ "retry" \notin seen
    /\ used' = used + IF Fault = "repeat-delta" THEN FinalSize - 4 ELSE 0
    /\ seen' = seen \cup {"retry"}
    /\ UNCHANGED <<names, sizes, diskNames, diskSizes, privateBytes,
                   phase, receipt, up, crashes, before, after>>

(* Persistence ordering is proved in the mutation models. Here writeback may
   retain a partial append or move, and quota rebuild counts public inodes once. *)
Writeback ==
    /\ up /\ receipt = "pending"
    /\ diskNames' = names /\ diskSizes' = sizes
    /\ UNCHANGED <<names, sizes, used, privateBytes, phase, receipt,
                   up, crashes, before, after, seen>>

Crash ==
    /\ up /\ crashes < MaxCrashes
    /\ up' = FALSE /\ crashes' = crashes + 1
    /\ names' = diskNames /\ sizes' = diskSizes /\ phase' = "offline"
    /\ seen' = IF phase = "account" THEN seen \cup {"crash-before-account"} ELSE seen
    /\ UNCHANGED <<diskNames, diskSizes, used, privateBytes, receipt, before, after>>

Reopen ==
    /\ ~up /\ up' = TRUE
    /\ used' = IF Fault = "skip-rebuild" THEN used ELSE Usage(names, sizes)
    /\ phase' = IF receipt = "pending" THEN "initial" ELSE "done"
    /\ seen' = seen \cup {"reopen"}
    /\ UNCHANGED <<names, sizes, diskNames, diskSizes, privateBytes, receipt,
                   crashes, before, after>>

Step == CASE phase = "initial" -> Stage
          [] phase = "publish" -> Publish \/ Refuse \/ Sync
          [] phase = "account" -> Account
          [] phase = "sync-replacement" -> Sync
          [] phase = "receipt" -> SaveReceipt
          [] OTHER -> FALSE
Next == Step \/ Retry \/ Writeback \/ Crash \/ Reopen
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Step) /\ WF_vars(Reopen)
Progress == <>(receipt # "pending")

TypeOK == /\ names \in [Paths -> Files \cup {"absent"}]
          /\ sizes \in [Files -> 0..4] /\ used \in Int
          /\ privateBytes \in {0, 10} /\ crashes \in 0..MaxCrashes
PublicUsage == up /\ phase # "account" => used = Usage(names, sizes)
QuotaAdmission == up /\ phase # "account" => used <= IF Limit < 4 THEN 4 ELSE Limit
NeverWitness == CASE Witness = "private-free" -> ~(privateBytes > Limit /\ used = 4 /\ up)
                  [] Witness = "crash-gap" -> ~({"crash-before-account", "reopen"} \subseteq seen)
                  [] OTHER -> TRUE
=============================================================================
