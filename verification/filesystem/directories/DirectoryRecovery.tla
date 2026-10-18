------------------------- MODULE DirectoryRecovery -------------------------
EXTENDS Naturals, TLC
CONSTANTS Operation, MaxCrashes, Fault, Witness
ASSUME /\ Operation \in {"mkdir", "rmdir"}
       /\ Fault \in {"none", "early-receipt", "retry-completed", "early-error"}
       /\ MaxCrashes \in Nat
VARIABLE s
vars == <<s>>
Initial == IF Operation = "mkdir" THEN "absent" ELSE "original"
Desired == IF Operation = "mkdir" THEN "created" ELSE "absent"
Init == s = [sql |-> "absent", name |-> Initial, disk |-> Initial,
             directorySynced |-> FALSE, parentSynced |-> FALSE,
             up |-> TRUE, crashes |-> 0, returned |-> FALSE,
             reused |-> FALSE, seen |-> {}]
Intent == /\ s.up /\ s.sql = "absent"
          /\ s' = [s EXCEPT !.sql = "pending"]
Apply == /\ s.up /\ s.sql = "pending" /\ s.name = Initial
         /\ s' = [s EXCEPT !.name = Desired, !.parentSynced = FALSE,
                           !.seen = @ \cup {"applied"}]
SyncDirectory == /\ s.up /\ s.sql = "pending" /\ s.name = Desired
                 /\ Operation = "mkdir"
                 /\ s' = [s EXCEPT !.directorySynced = TRUE]
SyncParent == /\ s.up /\ s.sql = "pending" /\ s.name = Desired
              /\ (Operation = "rmdir" \/ s.directorySynced)
              /\ s' = [s EXCEPT !.disk = s.name, !.parentSynced = TRUE]
Receipt == /\ s.up /\ s.sql = "pending" /\ s.name = Desired
           /\ ((s.parentSynced /\ (Operation = "rmdir" \/ s.directorySynced)) \/ Fault = "early-receipt")
           /\ s' = [s EXCEPT !.sql = "done"]
Error == /\ s.up /\ s.sql = "pending" /\ "applied" \in s.seen
         /\ Fault = "early-error"
         /\ s' = [s EXCEPT !.sql = "failed", !.returned = TRUE]
Return == /\ s.up /\ s.sql = "done"
          /\ s' = [s EXCEPT !.returned = TRUE]
Reuse == /\ s.up /\ s.sql = "done" /\ s.returned /\ ~s.reused
         /\ s' = [s EXCEPT !.name = "newer", !.disk = "newer", !.reused = TRUE]
Replay == /\ s.up /\ s.sql = "done" /\ s.reused
          /\ s' = [s EXCEPT !.name = IF Fault = "retry-completed" THEN Desired ELSE @,
                            !.seen = @ \cup {"replay-after-reuse"}]
Writeback == /\ s.sql = "pending"
             /\ s' = [s EXCEPT !.disk = s.name]
Crash == /\ s.up /\ s.crashes < MaxCrashes
         /\ s' = [s EXCEPT !.up = FALSE, !.crashes = @ + 1, !.name = s.disk,
                           !.directorySynced = FALSE, !.parentSynced = FALSE,
                           !.seen = @ \cup
                             (IF s.sql = "pending" /\ s.disk = Desired THEN {"gap"} ELSE {})
                             \cup (IF s.sql = "done" /\ ~s.returned THEN {"lost-ack"} ELSE {})]
Restart == /\ ~s.up /\ s' = [s EXCEPT !.up = TRUE]
(* One owner keeps the path unchanged by other calls while this operation is
   pending. Prepared mkdir may accept its own created directory; prepared rmdir
   may accept absence. Prepared carries no fields: replay supplies the path,
   and an existing removal target supplies its file/directory kind. No recorded
   path, inode identity, type or private link is needed.
   Intent abstracts Initial -> Prepared. Applying a name, syncing the directory
   and parent, and saving Completed remain separate crashable steps. *)
DecodeState == CASE s.sql = "absent" -> "Initial"
                 [] s.sql = "pending" -> "Prepared"
                 [] s.sql = "done" -> "Completed"
                 [] OTHER -> "Failed"
DirectoryLoop ==
    CASE DecodeState = "Initial" -> Intent
      [] DecodeState = "Prepared" -> Apply \/ SyncDirectory \/ SyncParent \/ Receipt \/ Error
      [] DecodeState = "Completed" -> Return \/ Replay
      [] OTHER -> FALSE
Next == DirectoryLoop \/ Reuse \/ Writeback \/ Crash \/ Restart
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Intent) /\ WF_vars(Apply) /\ WF_vars(SyncDirectory)
             /\ WF_vars(SyncParent) /\ WF_vars(Receipt) /\ WF_vars(Return)
             /\ WF_vars(Restart)
TypeOK == /\ s.sql \in {"absent", "pending", "done", "failed"}
          /\ s.name \in {"absent", "original", "created", "newer"}
          /\ s.disk \in {"absent", "original", "created", "newer"}
          /\ s.crashes \in 0..MaxCrashes
ReceiptDurable == s.sql = "done" /\ ~s.reused => s.disk = Desired
NewerUntouched == s.reused => s.name = "newer"
NoUncertainError == s.sql = "failed" => "applied" \notin s.seen
ReturnAfterReceipt == s.returned => s.sql = "done"
Progress == <>s.returned
NeverWitness == CASE Witness = "gap" -> ~(s.returned /\ "gap" \in s.seen)
                   [] Witness = "lost-ack" -> ~(s.returned /\ "lost-ack" \in s.seen)
                   [] OTHER -> "replay-after-reuse" \notin s.seen
=============================================================================
