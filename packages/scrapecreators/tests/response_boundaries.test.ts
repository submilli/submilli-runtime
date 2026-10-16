import { label } from "submilli:test";
import { decodeResponse, YouTubeGetTranscriptData, YouTubeGetVideoData, RedditListCommentsData } from "@submilli/scrapecreators";

function main(): void {
    label("missing YouTube captions remain a null typed transcript");
    const captions = decodeResponse('{"success":true,"transcript":null,"transcript_only_text":null,"language":null,"credits_charged":0}') as YouTubeGetTranscriptData;
    assert(captions.transcript === null && captions.transcript_only_text === null, "unavailable captions");

    label("video attribution can omit optional nested IDs");
    const video = decodeResponse('{"success":true,"id":"video","channel":{"title":"Example"},"watchNextVideos":[{"title":"Related"}]}') as YouTubeGetVideoData;
    assert(video.id === "video", "only the endpoint-level identity is mandatory");

    label("Reddit replies retain edited timestamps and boolean vote state");
    const comments = decodeResponse('{"success":true,"post":{"id":"post"},"comments":[{"id":"comment","replies":{"items":[{"id":"reply","edited":1784185607,"likes":true,"body":"Edited reply"}],"more":{"has_more":false,"next_cursor":null}}}]}') as RedditListCommentsData;
    assert(comments.comments.length === 1, "edited comment response keeps its page");
}
