package com.example.subroutine_simple.data.network

import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.ActionTemplate
import com.example.subroutine_simple.data.models.AccountInfo
import com.example.subroutine_simple.data.models.AllData
import com.example.subroutine_simple.data.models.CompleteResult
import com.example.subroutine_simple.data.models.DataDelta
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.EventTemplate
import com.example.subroutine_simple.data.models.Marker
import com.example.subroutine_simple.data.models.MarkerTemplate
import com.example.subroutine_simple.data.models.MutationReceipt
import com.example.subroutine_simple.data.models.MutationRequest
import com.example.subroutine_simple.data.models.Routine
import com.example.subroutine_simple.data.models.Signal
import com.example.subroutine_simple.data.models.SignalTemplate
import kotlinx.serialization.Serializable
import retrofit2.http.Body
import retrofit2.http.DELETE
import retrofit2.http.GET
import retrofit2.http.POST
import retrofit2.http.PUT
import retrofit2.http.Path
import retrofit2.http.Query

@Serializable
data class PinRequest(val pinned: Boolean)

@Serializable
data class InstantiateRequest(
    @kotlinx.serialization.SerialName("start_time") val startTime: String? = null,
)

interface SubroutineApi {
    @GET("v1/account")
    suspend fun getAccount(): AccountInfo

    @GET("v1/data")
    suspend fun getAllData(): AllData

    @GET("v1/data")
    suspend fun getDataDelta(@Query("since") since: Long): DataDelta

    @POST("v1/mutations")
    suspend fun submitMutation(@Body request: MutationRequest): MutationReceipt

    @GET("v1/actions")
    suspend fun listActions(): List<Action>

    @POST("v1/actions")
    suspend fun createAction(@Body action: Action): Action

    @GET("v1/actions/{id}")
    suspend fun getAction(@Path("id") id: String): Action

    @PUT("v1/actions/{id}")
    suspend fun upsertAction(@Path("id") id: String, @Body action: Action): Action

    @DELETE("v1/actions/{id}")
    suspend fun deleteAction(@Path("id") id: String)

    @POST("v1/actions/{id}/complete")
    suspend fun completeAction(@Path("id") id: String): CompleteResult

    @POST("v1/actions/{id}/queue")
    suspend fun queueAction(@Path("id") id: String): List<Action>

    @POST("v1/actions/{id}/backlog")
    suspend fun backlogAction(@Path("id") id: String): Action

    @POST("v1/actions/{id}/pin")
    suspend fun pinAction(@Path("id") id: String, @Body request: PinRequest): Action

    @POST("v1/actions/{id}/save")
    suspend fun saveAction(@Path("id") id: String): ActionTemplate

    @POST("v1/actions/{id}/clear_duration")
    suspend fun clearActionDuration(@Path("id") id: String): Action

    @GET("v1/actions/templates")
    suspend fun listActionTemplates(): List<ActionTemplate>

    @POST("v1/actions/templates")
    suspend fun createActionTemplate(@Body template: ActionTemplate): ActionTemplate

    @GET("v1/actions/templates/{id}")
    suspend fun getActionTemplate(@Path("id") id: String): ActionTemplate

    @PUT("v1/actions/templates/{id}")
    suspend fun upsertActionTemplate(
        @Path("id") id: String,
        @Body template: ActionTemplate,
    ): ActionTemplate

    @DELETE("v1/actions/templates/{id}")
    suspend fun deleteActionTemplate(@Path("id") id: String)

    @GET("v1/events")
    suspend fun listEvents(): List<Event>

    @POST("v1/events")
    suspend fun createEvent(@Body event: Event): Event

    @GET("v1/events/{id}")
    suspend fun getEvent(@Path("id") id: String): Event

    @PUT("v1/events/{id}")
    suspend fun upsertEvent(@Path("id") id: String, @Body event: Event): Event

    @DELETE("v1/events/{id}")
    suspend fun deleteEvent(@Path("id") id: String)

    @POST("v1/events/{id}/save")
    suspend fun saveEvent(@Path("id") id: String): EventTemplate

    @GET("v1/events/templates")
    suspend fun listEventTemplates(): List<EventTemplate>

    @POST("v1/events/templates")
    suspend fun createEventTemplate(@Body template: EventTemplate): EventTemplate

    @GET("v1/events/templates/{id}")
    suspend fun getEventTemplate(@Path("id") id: String): EventTemplate

    @PUT("v1/events/templates/{id}")
    suspend fun upsertEventTemplate(
        @Path("id") id: String,
        @Body template: EventTemplate,
    ): EventTemplate

    @DELETE("v1/events/templates/{id}")
    suspend fun deleteEventTemplate(@Path("id") id: String)

    @GET("v1/routines")
    suspend fun listRoutines(): List<Routine>

    @POST("v1/routines")
    suspend fun createRoutine(@Body routine: Routine): Routine

    @GET("v1/routines/{id}")
    suspend fun getRoutine(@Path("id") id: String): Routine

    @PUT("v1/routines/{id}")
    suspend fun upsertRoutine(@Path("id") id: String, @Body routine: Routine): Routine

    @DELETE("v1/routines/{id}")
    suspend fun deleteRoutine(@Path("id") id: String)

    @POST("v1/routines/{id}/instantiate")
    suspend fun instantiateRoutine(
        @Path("id") id: String,
        @Body request: InstantiateRequest,
    ): List<Action>

    @GET("v1/markers")
    suspend fun listMarkers(): List<Marker>

    @POST("v1/markers")
    suspend fun createMarker(@Body marker: Marker): Marker

    @GET("v1/markers/{id}")
    suspend fun getMarker(@Path("id") id: String): Marker

    @PUT("v1/markers/{id}")
    suspend fun upsertMarker(@Path("id") id: String, @Body marker: Marker): Marker

    @DELETE("v1/markers/{id}")
    suspend fun deleteMarker(@Path("id") id: String)

    @POST("v1/markers/{id}/save")
    suspend fun saveMarker(@Path("id") id: String): MarkerTemplate

    @GET("v1/markers/templates")
    suspend fun listMarkerTemplates(): List<MarkerTemplate>

    @POST("v1/markers/templates")
    suspend fun createMarkerTemplate(@Body template: MarkerTemplate): MarkerTemplate

    @GET("v1/markers/templates/{id}")
    suspend fun getMarkerTemplate(@Path("id") id: String): MarkerTemplate

    @PUT("v1/markers/templates/{id}")
    suspend fun upsertMarkerTemplate(
        @Path("id") id: String,
        @Body template: MarkerTemplate,
    ): MarkerTemplate

    @DELETE("v1/markers/templates/{id}")
    suspend fun deleteMarkerTemplate(@Path("id") id: String)

    @GET("v1/signals")
    suspend fun listSignals(): List<Signal>

    @POST("v1/signals")
    suspend fun createSignal(@Body signal: Signal): Signal

    @GET("v1/signals/{id}")
    suspend fun getSignal(@Path("id") id: String): Signal

    @PUT("v1/signals/{id}")
    suspend fun upsertSignal(@Path("id") id: String, @Body signal: Signal): Signal

    @DELETE("v1/signals/{id}")
    suspend fun deleteSignal(@Path("id") id: String)

    @POST("v1/signals/{id}/save")
    suspend fun saveSignal(@Path("id") id: String): SignalTemplate

    @GET("v1/signals/templates")
    suspend fun listSignalTemplates(): List<SignalTemplate>

    @POST("v1/signals/templates")
    suspend fun createSignalTemplate(@Body template: SignalTemplate): SignalTemplate

    @GET("v1/signals/templates/{id}")
    suspend fun getSignalTemplate(@Path("id") id: String): SignalTemplate

    @PUT("v1/signals/templates/{id}")
    suspend fun upsertSignalTemplate(
        @Path("id") id: String,
        @Body template: SignalTemplate,
    ): SignalTemplate

    @DELETE("v1/signals/templates/{id}")
    suspend fun deleteSignalTemplate(@Path("id") id: String)

    @POST("v1/pipeline/refresh")
    suspend fun refreshPipeline(): List<Action>

    @POST("v1/pipeline/expedite")
    suspend fun expeditePipeline(): List<Action>
}
